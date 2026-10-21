//! Recovering failed plans and handing off a task prefix to the planner.

use super::finish::task_usage;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use tinycomputer_bus::FLOW_GUIDE;
use tinycomputer_bus::agent::{SurfaceKind, TaskConstraints, TaskId, TaskStatus};

use super::drive;
use crate::task::publish::publish;
use crate::task::store::{Cell, Run};
use crate::task::FlowRunner;

pub(in crate::task) async fn handle_plan_failure(
    cell: Arc<Cell>,
    runner: Arc<dyn FlowRunner>,
    id: TaskId,
    task: String,
    reason: String,
) {
    if let Err(recovery) =
        recover_plan_after_planner_failure(cell.clone(), runner.clone(), id.clone(), task).await
    {
        runner.release(&id);
        let failure = format!("{reason}; host planning recovery failed: {recovery}");
        publish(
            &cell,
            TaskStatus::Failed {
                step: None,
                reason: failure.clone(),
                hint: "reword the task, or pass a flow written with Describe's guide".to_owned(),
                recoverable: true,
            },
            &format!("Planning failed: {failure}"),
        );
    }
}

async fn recover_plan_after_planner_failure(
    cell: Arc<Cell>,
    runner: Arc<dyn FlowRunner>,
    id: TaskId,
    task: String,
) -> Result<(), String> {
    let plan =
        crate::task::plan_recovery::host_plan(&cell, runner.as_ref(), task, FLOW_GUIDE.to_owned())
            .await?;
    let facts = cell
        .state
        .lock()
        .map_err(|_| "task state is unavailable".to_owned())?
        .facts
        .clone();
    let known = crate::task::names::known_names(&plan, &facts);
    let fact_names = crate::task::names::fact_names(&facts);
    let missing = crate::agentic::missing_inputs(&plan, &known, &fact_names);
    let allow = {
        let mut state = cell
            .state
            .lock()
            .map_err(|_| "task state is unavailable".to_owned())?;
        state.flow = plan.clone();
        state.constraints.allow_destructive
    };
    if !missing.is_empty() {
        runner.release(&id);
        publish(
            &cell,
            crate::task::publish::needs_input(&missing),
            "The recovered plan needs caller values before it can start.",
        );
        return Ok(());
    }
    drive(
        cell,
        runner,
        vec![Run {
            flow: plan,
            allow_destructive: allow,
            rescue: None,
        }],
    )
    .await;
    Ok(())
}

pub(in crate::task) async fn prepare_page(
    runner: Arc<dyn FlowRunner>,
    id: tinycomputer_bus::agent::TaskId,
    constraints: TaskConstraints,
    page: Option<String>,
    prepare_runner: bool,
) {
    if prepare_runner {
        runner.prepare(&id, &constraints).await;
        if let Some(page) = page {
            runner.open_page(&id, &page).await;
        }
    }
}

/// Plans the rest of a task after a supplied flow prefix. The runner is not
/// prepared or opened here: it still owns the page the prefix left current.
pub(in crate::task) async fn plan_handoff(
    cell: &Arc<Cell>,
    runner: &dyn FlowRunner,
) -> Result<Option<tinycomputer_bus::Flow>, String> {
    let (task, surfaces, facts, planner) = {
        let state = cell
            .state
            .lock()
            .map_err(|_| "task state is unavailable".to_owned())?;
        (
            state.goal.clone(),
            state.constraints.surfaces.clone(),
            state.facts.clone(),
            cell.planner.clone(),
        )
    };
    if task.trim().is_empty() {
        return Err("the handoff task text is empty".to_owned());
    }
    let (plan, origin) =
        draft_from_current_page(cell, runner, task, surfaces, facts.clone(), planner).await?;
    validate_handoff_plan(cell, plan, &facts, &origin)
}

async fn draft_from_current_page(
    cell: &Arc<Cell>,
    runner: &dyn FlowRunner,
    task: String,
    surfaces: Vec<SurfaceKind>,
    facts: tinycomputer_core::Facts,
    planner: Option<crate::planner::Planner>,
) -> Result<(tinycomputer_bus::agent::TaskPlan, String), String> {
    let usage = task_usage(cell);
    let call_budget = usage
        .task_call_budget()
        .ok_or_else(|| "the task model-call budget is unavailable".to_owned())?;
    let budget_left = {
        let state = cell
            .state
            .lock()
            .map_err(|_| "task state is unavailable".to_owned())?;
        let calls_left = state
            .budget
            .max_model_calls
            .unwrap_or(crate::task::DEFAULT_MODEL_CALLS)
            .saturating_sub(state.spent.model_calls);
        let elapsed_left = state
            .budget
            .max_elapsed_ms
            .map(|max| max.saturating_sub(state.spent.elapsed_ms));
        if calls_left == 0 {
            return Err(
                "the task has no model calls remaining for continuation planning".to_owned(),
            );
        }
        if elapsed_left == Some(0) {
            return Err("the task has no time remaining for continuation planning".to_owned());
        }
        elapsed_left
    };
    // Drop also charges elapsed time and started planner calls if this future
    // is cancelled while the model is still responding.
    let mut debit = PlanningDebit::new(Arc::downgrade(cell));
    let id = cell.view.borrow().id.clone();
    let origin_future = runner.page_origin(&id);
    let origin = if let Some(ms) = budget_left {
        tokio::time::timeout(Duration::from_millis(ms), origin_future)
            .await
            .map_err(|_| "reading the current page exceeded the task time budget".to_owned())?
    } else {
        origin_future.await
    }
    .ok_or_else(|| "the current browser origin is unavailable".to_owned())?;
    let planning_task = runner.redact(
        &id,
        &format!(
            "{task}\n\n{}\nContinue this same task from the current page at {origin}. The current browser session is still open. Do not open or browse to the shop start page again, and do not repeat the completed prefix actions. Inspect the current page before choosing the next action.",
            successful_prefix_summary(cell)
        ),
    );
    let plan = run_handoff_planner(
        cell,
        runner,
        HandoffDraft {
            task: planning_task.clone(),
            surfaces,
            facts,
            planner,
            budget_left,
            call_budget,
        },
        &mut debit,
    )
    .await?;
    drop(debit);
    ensure_handoff_budget(cell)?;
    Ok((plan, origin))
}

fn successful_prefix_summary(cell: &Cell) -> String {
    let Ok(state) = cell.state.lock() else {
        return "The safe prefix completed; its action structure is unavailable.".to_owned();
    };
    let mut counts = BTreeMap::<&'static str, usize>::new();
    for report in &state.steps {
        if !matches!(
            report.outcome,
            tinycomputer_bus::StepOutcome::Done | tinycomputer_bus::StepOutcome::AlreadyDone
        ) {
            continue;
        }
        let category = match report.kind.as_str() {
            "browse" => "navigation",
            "do" => "browser interaction",
            "wait_for" => "page wait",
            "verify" => "page check",
            _ => continue,
        };
        *counts.entry(category).or_default() += 1;
    }
    if counts.is_empty() {
        return "The safe recipe prefix completed; no successful replayable action classes were recorded.".to_owned();
    }
    let steps = counts
        .into_iter()
        .map(|(kind, count)| format!("{count} completed {kind} step(s)"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Completed safe-prefix structure: {steps}. Do not repeat these steps.")
}

async fn run_handoff_planner(
    cell: &Arc<Cell>,
    runner: &dyn FlowRunner,
    draft: HandoffDraft,
    debit: &mut PlanningDebit,
) -> Result<tinycomputer_bus::agent::TaskPlan, String> {
    let planning = draft_handoff(
        cell,
        runner,
        draft.task.clone(),
        draft.surfaces,
        draft.facts,
        draft.planner,
        draft.call_budget.clone(),
    );
    let result = if let Some(ms) = draft.budget_left {
        let remaining = ms
            .saturating_sub(u64::try_from(debit.started.elapsed().as_millis()).unwrap_or(u64::MAX));
        tokio::time::timeout(Duration::from_millis(remaining), planning)
            .await
            .map_err(|_| "continuation planning exceeded the task time budget".to_owned())?
    } else {
        planning.await
    };
    match result {
        Ok(plan) => Ok(plan),
        Err(reason) if runner.plan_recovery_enabled() => {
            debit.charge();
            let remaining_calls = {
                let state = cell
                    .state
                    .lock()
                    .map_err(|_| "task state is unavailable".to_owned())?;
                state
                    .budget
                    .max_model_calls
                    .unwrap_or(crate::task::DEFAULT_MODEL_CALLS)
                    .saturating_sub(state.spent.model_calls)
            };
            if remaining_calls == 0 {
                return Err(reason);
            }
            let flow = crate::task::plan_recovery::host_plan(
                cell,
                runner,
                draft.task,
                FLOW_GUIDE.to_owned(),
            )
            .await
            .map_err(|recovery| format!("{reason}; host recovery failed: {recovery}"))?;
            Ok(tinycomputer_bus::agent::TaskPlan {
                flow,
                questions: Vec::new(),
                notes: Vec::new(),
            })
        }
        Err(reason) => Err(reason),
    }
}

struct HandoffDraft {
    task: String,
    surfaces: Vec<SurfaceKind>,
    facts: tinycomputer_core::Facts,
    planner: Option<crate::planner::Planner>,
    budget_left: Option<u64>,
    call_budget: crate::task::TaskCallBudget,
}

fn ensure_handoff_budget(cell: &Cell) -> Result<(), String> {
    let state = cell
        .state
        .lock()
        .map_err(|_| "task state is unavailable".to_owned())?;
    if state.spent.model_calls
        > state
            .budget
            .max_model_calls
            .unwrap_or(crate::task::DEFAULT_MODEL_CALLS)
    {
        return Err("continuation planning exhausted the task model call budget".to_owned());
    }
    if state
        .budget
        .max_elapsed_ms
        .is_some_and(|max| state.spent.elapsed_ms > max)
    {
        return Err("continuation planning exhausted the task time budget".to_owned());
    }
    Ok(())
}

fn validate_handoff_plan(
    cell: &Arc<Cell>,
    plan: tinycomputer_bus::agent::TaskPlan,
    facts: &tinycomputer_core::Facts,
    origin: &str,
) -> Result<Option<tinycomputer_bus::Flow>, String> {
    let known = crate::task::names::known_names(&plan.flow, facts);
    let fact_names = crate::task::names::fact_names(facts);
    let errors = crate::agentic::check_flow(&plan.flow, &known, &fact_names)
        .errors
        .into_iter()
        .filter(|error| !crate::task::names::is_undefined(error))
        .collect::<Vec<_>>();
    if plan.flow.steps.is_empty() || !errors.is_empty() {
        return Err(if plan.flow.steps.is_empty() {
            "the planner returned an empty continuation".to_owned()
        } else {
            format!(
                "the planner returned an invalid continuation: {}",
                errors.join("; ")
            )
        });
    }
    if reopens_origin(&plan.flow, origin) {
        return Err("the planner tried to reopen the current shop's start page".to_owned());
    }
    let missing = crate::agentic::missing_inputs(&plan.flow, &known, &fact_names);
    let waiting = if plan.questions.is_empty() {
        (!missing.is_empty()).then(|| crate::task::publish::needs_input(&missing))
    } else {
        Some(TaskStatus::NeedsInput {
            fields: plan.questions,
        })
    };
    if let Ok(mut state) = cell.state.lock() {
        state.flow = plan.flow.clone();
    }
    if let Some(status) = waiting {
        publish(
            cell,
            status,
            "The remaining task needs values before it can continue.",
        );
        return Ok(None);
    }
    Ok(Some(plan.flow))
}

struct PlanningDebit {
    cell: Weak<Cell>,
    started: Instant,
    charged: bool,
}

impl PlanningDebit {
    fn new(cell: Weak<Cell>) -> Self {
        Self {
            cell,
            started: Instant::now(),
            charged: false,
        }
    }

    fn charge(&mut self) {
        if self.charged {
            return;
        }
        self.charged = true;
        if let Some(cell) = self.cell.upgrade()
            && let Ok(mut state) = cell.state.lock()
        {
            state.spent.elapsed_ms = state.spent.elapsed_ms.saturating_add(
                u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
        }
    }
}

impl Drop for PlanningDebit {
    fn drop(&mut self) {
        self.charge();
    }
}

fn reopens_origin(flow: &tinycomputer_bus::Flow, origin: &str) -> bool {
    let Ok(origin) = url::Url::parse(origin) else {
        return true;
    };
    if origin.scheme() != "https" || !origin.username().is_empty() || origin.password().is_some() {
        return true;
    }
    reopens_origin_in_steps(&flow.steps, &origin)
}

fn reopens_origin_in_steps(steps: &[tinycomputer_bus::FlowStep], origin: &url::Url) -> bool {
    use tinycomputer_bus::FlowAction;
    steps.iter().any(|step| match step.action() {
        FlowAction::Browse(address) => {
            let Ok(url) = url::Url::parse(&address) else {
                return false;
            };
            url.scheme() == "https"
                && url.host_str() == origin.host_str()
                && url.port_or_known_default() == origin.port_or_known_default()
                && url.username().is_empty()
                && url.password().is_none()
                && is_front_page(url.path())
        }
        FlowAction::If(branch) => {
            reopens_origin_in_steps(&branch.then, origin)
                || reopens_origin_in_steps(&branch.otherwise, origin)
        }
        FlowAction::RepeatUntil(repeat) => reopens_origin_in_steps(&repeat.steps, origin),
        _ => false,
    })
}

fn is_front_page(path: &str) -> bool {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty());
    let segments = segments.collect::<Vec<_>>();
    segments.len() <= 2
        && segments.iter().all(|segment| {
            let bytes = segment.as_bytes();
            (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_alphabetic))
                || (bytes.len() == 5
                    && bytes[..2].iter().all(u8::is_ascii_alphabetic)
                    && matches!(bytes[2], b'-' | b'_')
                    && bytes[3..].iter().all(u8::is_ascii_alphabetic))
        })
}

async fn draft_handoff(
    cell: &Arc<Cell>,
    runner: &dyn FlowRunner,
    task: String,
    surfaces: Vec<SurfaceKind>,
    facts: tinycomputer_core::Facts,
    planner: Option<crate::planner::Planner>,
    call_budget: crate::task::TaskCallBudget,
) -> Result<tinycomputer_bus::agent::TaskPlan, String> {
    if let Some(planner) = planner {
        let planner = planner.with_usage_recorder(task_usage(cell));
        let started = Instant::now();
        let names = facts
            .names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let secrets = facts
            .secret_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let reserve_call = || call_budget.reserve_model_call();
        let (outcome, used) = planner
            .plan_measured_with_budget(&task, &names, &secrets, &surfaces, Some(&reserve_call))
            .await;
        let id = cell.view.borrow().id.clone();
        runner.journal(Some(&id), "plan", &|| {
            crate::task::timing::planned(&outcome, used, started.elapsed(), planner.configuration())
        });
        match outcome {
            Ok(plan) => Ok(plan),
            Err(reason) => Err(reason),
        }
    } else if runner.plan_recovery_enabled() {
        let flow = crate::task::plan_recovery::host_plan(cell, runner, task, FLOW_GUIDE.to_owned())
            .await?;
        Ok(tinycomputer_bus::agent::TaskPlan {
            flow,
            questions: Vec::new(),
            notes: Vec::new(),
        })
    } else {
        Err("no planner or host plan-recovery hook is configured".to_owned())
    }
}
