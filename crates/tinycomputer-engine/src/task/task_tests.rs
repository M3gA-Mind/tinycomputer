//! Tests for the task controller over scripted flow runs.
//!
//! The runner hands back queued `RunFlow` replies and records every request,
//! so each pause, resume, and failure path is exercised without Jev or a
//! surface. This root holds that runner and the helpers every topic in
//! `task_tests/` shares.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod approval_tests;
mod artifact_tests;
mod describe_tests;
mod errors_tests;
mod human_tests;
mod output_tests;
mod page_tests;
mod persistence_tests;
mod plan_tests;
mod rescue_tests;
mod runner_tests;
mod start_tests;
mod status_tests;
mod timing_tests;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tinycomputer_bus::agent::{
    AgentResponse, AwaitTaskRequest, ContinueTaskRequest, InputKind, PaymentMode, StartTaskRequest,
    TaskConstraints, TaskId, TaskStatus, TaskView,
};
use tinycomputer_bus::{
    DesktopError, DesktopResponse, Flow, FlowAction, FlowRunResult, FlowStep, FlowStopReason,
    IfStep, JevMetrics, JevTarget, RunFlowRequest, StepOutcome, StepReport,
};

use super::interpret::app_at;
use super::publish::next_calls;
use super::{FlowFuture, FlowRunner, MAX_TASKS, Tasks, capabilities, input_kind};

/// Replies queued in order; a missing reply never resolves, like a flow
/// still running.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<DesktopResponse>>,
    requests: Mutex<Vec<RunFlowRequest>>,
    constraints: Mutex<Vec<TaskConstraints>>,
    host_secrets: Mutex<Vec<String>>,
    /// What the task's surface shows when asked.
    screen: Mutex<Vec<String>>,
    /// Tasks let go of, in order.
    released: Mutex<Vec<TaskId>>,
    /// The screenshot `capture` hands back, if any.
    shot: Mutex<Option<tinycomputer_bus::browser::OutputRef>>,
    /// A capture that never answers, like a hung surface.
    stuck: std::sync::atomic::AtomicBool,
    /// `capture`, `release`, `prepare` and `open_page` calls, in the order
    /// they arrived.
    events: Mutex<Vec<&'static str>>,
    /// What the task journaled outside its flows, in order.
    journaled: Mutex<Vec<(Option<TaskId>, String, serde_json::Value)>>,
    /// Tasks whose surfaces were got ready while they were planned.
    prepared: Mutex<Vec<TaskId>>,
    /// Tasks whose Jev connections were warmed while they were planned,
    /// with the votes asked for. The warm-up never ends.
    warmed: Mutex<Vec<(TaskId, u32)>>,
    /// Optional value a test's `FlowRunner` redacts from task-level outputs.
    host_secret: Mutex<Option<String>>,
    /// The pages loaded while tasks were planned, with their task.
    opened: Mutex<Vec<(TaskId, String)>>,
}

impl FlowRunner for Script {
    fn run(
        &self,
        _task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
    ) -> FlowFuture {
        self.requests.lock().unwrap().push(request);
        self.constraints.lock().unwrap().push(constraints.clone());
        let reply = self.replies.lock().unwrap().pop_front();
        Box::pin(async move {
            match reply {
                Some(reply) => reply,
                None => std::future::pending().await,
            }
        })
    }

    fn visible_text(&self, _task: &TaskId) -> super::TextFuture {
        let texts = self.screen.lock().unwrap().clone();
        Box::pin(async move { texts })
    }

    fn capture(&self, _task: &TaskId) -> super::CaptureFuture {
        self.events.lock().unwrap().push("capture");
        let shot = self.shot.lock().unwrap().clone();
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(std::future::pending());
        }
        Box::pin(async move { shot })
    }

    fn release(&self, task: &TaskId) {
        self.events.lock().unwrap().push("release");
        self.released.lock().unwrap().push(task.clone());
    }

    fn prepare(&self, task: &TaskId, _constraints: &TaskConstraints) -> super::PrepareFuture {
        self.events.lock().unwrap().push("prepare");
        self.prepared.lock().unwrap().push(task.clone());
        Box::pin(async {})
    }

    fn open_page(&self, task: &TaskId, url: &str) -> super::PrepareFuture {
        self.events.lock().unwrap().push("open_page");
        self.opened
            .lock()
            .unwrap()
            .push((task.clone(), url.to_owned()));
        Box::pin(async {})
    }

    fn warm(&self, task: &TaskId, votes: u32) -> super::PrepareFuture {
        self.warmed.lock().unwrap().push((task.clone(), votes));
        Box::pin(std::future::pending())
    }

    fn journal(&self, task: Option<&TaskId>, event: &str, fields: &dyn Fn() -> serde_json::Value) {
        self.journaled
            .lock()
            .unwrap()
            .push((task.cloned(), event.to_owned(), fields()));
    }

    fn redact(&self, _task: &TaskId, text: &str) -> String {
        let mut redacted = self.host_secrets
            .lock()
            .unwrap()
            .iter()
            .fold(text.to_owned(), |text, secret| text.replace(secret, "[redacted]"));
        if let Some(secret) = self.host_secret.lock().ok().and_then(|secret| secret.clone()) {
            redacted = redacted.replace(&secret, "•••• 1111");
        }
        redacted
    }
}

/// The `event`s the task journaled outside its flows, with the task each
/// went to.
fn journaled(script: &Script, event: &str) -> Vec<(Option<TaskId>, serde_json::Value)> {
    script
        .journaled
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, kind, _)| kind == event)
        .map(|(task, _, fields)| (task.clone(), fields.clone()))
        .collect()
}

fn controller(replies: Vec<DesktopResponse>) -> (Tasks, Arc<Script>) {
    let script = Arc::new(Script {
        replies: Mutex::new(replies.into()),
        ..Script::default()
    });
    (Tasks::new(script.clone()), script)
}

fn step(path: &str, kind: &str, text: &str, outcome: StepOutcome, note: &str) -> StepReport {
    StepReport {
        path: path.to_owned(),
        kind: kind.to_owned(),
        text: text.to_owned(),
        outcome,
        turns: 1,
        jev_calls: 1,
        actions: Vec::new(),
        loops: Vec::new(),
        confidence: None,
        note: note.to_owned(),
    }
}

fn finished_run(
    stop: FlowStopReason,
    steps: Vec<StepReport>,
    vars: &[(&str, &str)],
    pending: Option<&str>,
) -> DesktopResponse {
    let result = FlowRunResult {
        stop,
        steps,
        vars: vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        pending: pending.map(|name| JevTarget {
            ref_id: "e9".to_owned(),
            role: "button".to_owned(),
            name: Some(name.to_owned()),
        }),
        learned: Vec::new(),
        actions: 3,
        metrics: JevMetrics::default(),
        trace: Vec::new(),
        dialog_left_open: false,
    };
    DesktopResponse::ok("run-flow", serde_json::to_value(result).unwrap())
}

fn flow(value: serde_json::Value) -> Flow {
    serde_json::from_value(value).unwrap()
}

fn start(tasks: &Tasks, flow_value: serde_json::Value, facts: &[(&str, &str)]) -> TaskView {
    let reply = tasks.start(&StartTaskRequest {
        flow: Some(flow(flow_value)),
        facts: facts
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        ..StartTaskRequest::default()
    });
    assert!(reply.ok, "{:?}", reply.error);
    reply.data.unwrap()
}

async fn settle(tasks: &Tasks, id: &TaskId) -> TaskView {
    tasks
        .await_task(AwaitTaskRequest {
            id: id.clone(),
            timeout_ms: 5_000,
        })
        .await
        .data
        .unwrap()
}

fn code<T>(reply: &AgentResponse<T>) -> &str {
    &reply.error.as_ref().expect("an error").code
}

fn failed_at_step_two() -> DesktopResponse {
    finished_run(
        FlowStopReason::StepFailed,
        vec![
            step("1", "browse", "https://flights.test", StepOutcome::Done, ""),
            step(
                "2",
                "do",
                "search for flights",
                StepOutcome::Failed,
                "nothing to click",
            ),
        ],
        &[],
        None,
    )
}
