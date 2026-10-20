//! A task's budget across its runs: the request each run is given, capped at
//! what the task has not spent, and ending a task whose time ran out.

use std::collections::BTreeMap;

use tinycomputer_bus::RunFlowRequest;
use tinycomputer_bus::agent::{TaskConstraints, TaskStatus};

use super::artifact::captured;
use super::brief::brief;
use super::names::fact_names;
use super::publish::{publish, stopped_summary};
use super::store::{Cell, Run};
use super::{DEFAULT_MODEL_CALLS, DEFAULT_VOTES, FlowRunner};

/// Builds the next `RunFlowRequest` for `run`, capped at what the task's
/// budget has not already spent, along with the constraints to run it under
/// and how much of `max_elapsed_ms` remains (`None` when it is unbounded).
/// `None` overall only when the task's state was poisoned by a panic.
pub(super) fn run_request(
    cell: &Cell,
    run: &Run,
) -> Option<(RunFlowRequest, TaskConstraints, Option<u64>)> {
    let state = cell.state.lock().ok()?;
    // Only the caller's values: the flow's own definitions travel with the
    // flow, and the runtime expands them against these. The secret ones are
    // named in `facts`, so they never reach Jev but as `${name}`.
    let vars = state
        .facts
        .names()
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), state.facts.get(name)?.to_owned())))
        .collect::<BTreeMap<_, _>>();
    let facts = fact_names(&state.facts);
    let max_actions = state
        .budget
        .max_actions
        .unwrap_or(120)
        .saturating_sub(state.spent.actions);
    let max_model_calls = state
        .budget
        .max_model_calls
        .unwrap_or(DEFAULT_MODEL_CALLS)
        .saturating_sub(state.spent.model_calls);
    let time_left = state
        .budget
        .max_elapsed_ms
        .map(|max| max.saturating_sub(state.spent.elapsed_ms));
    Some((
        RunFlowRequest {
            flow: run.flow.clone(),
            vars,
            facts,
            fact_handles: state
                .facts
                .secret_names()
                .into_iter()
                .filter_map(|name| {
                    state.facts.handle(name).map(|handle| {
                        (
                            name.to_owned(),
                            tinycomputer_bus::agent::SecretHandle {
                                handle: handle.handle.clone(),
                                mask: handle.mask.clone(),
                                kind: handle.kind.clone(),
                            },
                        )
                    })
                })
                .collect(),
            task_id: Some(cell.view.borrow().id.0.clone()),
            allow_destructive: run.allow_destructive,
            // Jev must read what the fields hold to check what was typed.
            // Secret values are masked in everything the runtime sends Jev.
            include_values: true,
            max_actions,
            max_model_calls,
            votes: state.budget.votes.unwrap_or(DEFAULT_VOTES),
            strategy: state.budget.strategy.unwrap_or_default(),
            deliberation: state.budget.deliberation.unwrap_or_default(),
            brief: brief(&state),
            memory: state.memory.clone(),
            trace: state.trace,
            // What earlier runs saved, so a resumed run remembers it.
            collected: state.reads.clone(),
            dialog_left_open: state.dialog_left_open,
            ..RunFlowRequest::default()
        },
        state.constraints.clone(),
        time_left,
    ))
}

/// Ends a task outright with `status`, without a flow run to interpret: its
/// time budget ran out before a run of it could even start, or a run of it
/// had to be cut off mid-flight to keep from spending past what remains.
pub(super) async fn stop_task(cell: &Cell, runner: &dyn FlowRunner, status: TaskStatus) {
    // A task cut off by its time budget still leaves its last screen.
    let status = captured(cell, runner, status).await;
    let summary = stopped_summary(&status);
    publish(cell, status, &summary);
    runner.release(&cell.view.borrow().id);
}

/// The task-level failure `stop_task` reports when `budget.max_elapsed_ms`
/// is spent: never recoverable by a resume, the same as an action or model
/// budget running out inside a run.
pub(super) fn elapsed_budget_failed() -> TaskStatus {
    TaskStatus::Failed {
        step: None,
        reason: "the task's time budget ran out".to_owned(),
        hint: "raise budget.max_elapsed_ms".to_owned(),
        recoverable: true,
    }
}
