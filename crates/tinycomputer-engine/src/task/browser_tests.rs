//! Public task-manager behavior on a scripted browser-only runner.

use std::sync::Arc;

use tinycomputer_bus::agent::{
    AwaitTaskRequest, StartTaskRequest, SurfaceKind, TaskConstraints, TaskStatus,
};
use tinycomputer_bus::{
    DesktopResponse, FlowRunResult, FlowStopReason, JevMetrics, RunFlowRequest, StepOutcome,
    StepReport,
};
use tinycomputer_bus::{Flow, FlowAction, FlowStep};

use super::{FlowFuture, FlowRunner};
use crate::{Completion, LanguageModel, Rescuer, Tasks, Turn};

struct ScriptedBrowserRunner {
    failure: Option<&'static str>,
}

impl FlowRunner for ScriptedBrowserRunner {
    fn run(
        &self,
        _task: &tinycomputer_bus::agent::TaskId,
        constraints: &TaskConstraints,
        _request: RunFlowRequest,
    ) -> FlowFuture {
        assert_eq!(constraints.surfaces, [SurfaceKind::Browser]);
        let failure = self.failure;
        let result = FlowRunResult {
            stop: if failure.is_some() {
                FlowStopReason::StepFailed
            } else {
                FlowStopReason::Completed
            },
            steps: vec![StepReport {
                path: "1".to_owned(),
                kind: "browse".to_owned(),
                text: "open the local page".to_owned(),
                outcome: if failure.is_some() {
                    StepOutcome::Failed
                } else {
                    StepOutcome::Done
                },
                turns: 0,
                jev_calls: 0,
                actions: Vec::new(),
                loops: Vec::new(),
                confidence: None,
                note: failure.unwrap_or_default().to_owned(),
            }],
            vars: std::collections::BTreeMap::new(),
            pending: None,
            learned: Vec::new(),
            actions: 0,
            metrics: JevMetrics::default(),
            trace: Vec::new(),
            dialog_left_open: false,
        };
        let reply = serde_json::to_value(result).map_or_else(
            |error| {
                DesktopResponse::err(
                    "run-flow",
                    tinycomputer_bus::DesktopError::new("INTERNAL", error.to_string()),
                )
            },
            |value| DesktopResponse::ok("run-flow", value),
        );
        Box::pin(async move { reply })
    }
}

#[tokio::test]
async fn public_task_manager_runs_a_scripted_browser_flow_to_done_without_models()
-> Result<(), Box<dyn std::error::Error>> {
    let tasks = Tasks::new(Arc::new(ScriptedBrowserRunner { failure: None }));
    let started = tasks.start(&request());
    assert!(started.ok, "{:?}", started.error);
    let Some(started) = started.data else {
        return Err("the task start reply had no view".into());
    };

    let finished = settle(&tasks, &started.id).await?;
    assert!(matches!(finished.status, TaskStatus::Done { .. }));
    Ok(())
}

struct RefusalRecovery;

impl LanguageModel for RefusalRecovery {
    fn complete(&self, _turns: &[Turn]) -> Completion {
        Box::pin(async { Err("scripted recovery is unavailable".to_owned()) })
    }
}

#[tokio::test]
async fn a_browser_action_refusal_keeps_its_reason_in_rescue_and_task_report()
-> Result<(), Box<dyn std::error::Error>> {
    const REASON: &str = "the host refused this browser action";
    let tasks = Tasks::new(Arc::new(ScriptedBrowserRunner {
        failure: Some(REASON),
    }))
    .with_rescuer(Rescuer::new(Arc::new(RefusalRecovery)));
    let started = tasks.start(&request());
    assert!(started.ok, "{:?}", started.error);
    let Some(started) = started.data else {
        return Err("the task start reply had no view".into());
    };
    let finished = settle(&tasks, &started.id).await?;
    assert!(matches!(
        &finished.status,
        TaskStatus::Failed { reason, .. } if reason == REASON
    ));

    let report = tasks.report(&started.id);
    assert!(report.ok, "{:?}", report.error);
    let Some(report) = report.data else {
        return Err("the task report reply had no report".into());
    };
    assert_eq!(report.view.status, finished.status);
    assert_eq!(report.steps[0].note, REASON);
    assert_eq!(report.rescues[0].failure, REASON);
    Ok(())
}

fn request() -> StartTaskRequest {
    StartTaskRequest {
        flow: Some(Flow {
            app: "browser".to_owned(),
            vars: std::collections::BTreeMap::new(),
            steps: vec![FlowStep::Action(FlowAction::Browse(
                "http://127.0.0.1:8765/index.html".to_owned(),
            ))],
        }),
        constraints: TaskConstraints {
            surfaces: vec![SurfaceKind::Browser],
            ..TaskConstraints::default()
        },
        ..StartTaskRequest::default()
    }
}

async fn settle(
    tasks: &Tasks,
    id: &tinycomputer_bus::agent::TaskId,
) -> Result<tinycomputer_bus::agent::TaskView, Box<dyn std::error::Error>> {
    loop {
        let reply = tasks
            .await_task(AwaitTaskRequest {
                id: id.clone(),
                timeout_ms: 1_000,
            })
            .await;
        assert!(reply.ok, "{:?}", reply.error);
        let Some(view) = reply.data else {
            return Err("the await reply had no task view".into());
        };
        if view.status.is_final() {
            return Ok(view);
        }
    }
}
