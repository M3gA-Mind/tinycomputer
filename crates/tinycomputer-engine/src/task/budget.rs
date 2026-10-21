//! A task's budget across its runs: the request each run is given, capped at
//! what the task has not spent, and ending a task whose time ran out.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;

use tinycomputer_bus::RunFlowRequest;
use tinycomputer_bus::agent::{TaskConstraints, TaskStatus};

use super::artifact::captured;
use super::brief::brief;
use super::names::fact_names;
use super::publish::{publish, stopped_summary};
use super::store::{Cell, Run};
use super::{DEFAULT_MODEL_CALLS, DEFAULT_VOTES, FlowRunner};

/// A handle to reserve host model calls against this task's Jev evaluation
/// budget. Hosts must reserve before starting provider I/O; a reservation is
/// not refunded when that I/O fails or is cancelled.
#[derive(Clone)]
pub struct TaskCallBudget {
    cell: Weak<Cell>,
}

impl TaskCallBudget {
    pub(super) fn for_cell(cell: Weak<Cell>) -> Self {
        Self { cell }
    }

    /// Starts one host-work interval whose active time counts against the
    /// task's elapsed budget until its last clone is dropped.
    #[must_use]
    pub fn begin_work(&self) -> HostWorkGuard {
        HostWorkGuard {
            inner: Arc::new(HostWork {
                cell: self.cell.clone(),
                started: Instant::now(),
                charged: AtomicBool::new(false),
            }),
        }
    }
}

impl fmt::Debug for TaskCallBudget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskCallBudget")
            .finish_non_exhaustive()
    }
}

/// One cloneable host-work interval, shared with the request passed to the
/// host callback. It accounts elapsed time once, after the last clone drops.
#[derive(Clone)]
pub struct TaskBudgetScope {
    inner: Arc<HostWork>,
}

/// Owns one host-work interval. The task is charged when this guard is dropped,
/// even if a callback retained a clone of its [`TaskBudgetScope`].
pub struct HostWorkGuard {
    inner: Arc<HostWork>,
}

impl HostWorkGuard {
    /// A cloneable view a host callback can retain and use for live checks.
    #[must_use]
    pub fn scope(&self) -> TaskBudgetScope {
        TaskBudgetScope {
            inner: self.inner.clone(),
        }
    }
}

impl fmt::Debug for HostWorkGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostWorkGuard")
            .finish_non_exhaustive()
    }
}

impl Drop for HostWorkGuard {
    fn drop(&mut self) {
        self.inner.charge();
    }
}

impl TaskBudgetScope {
    /// Reserves one task model call before the host starts a provider request.
    /// The reservation remains spent if the request fails or is cancelled.
    #[must_use]
    pub fn try_reserve_model_call(&self) -> bool {
        let Some(cell) = self.inner.cell.upgrade() else {
            return false;
        };
        let Ok(mut state) = cell.state.lock() else {
            return false;
        };
        let max = state.budget.max_model_calls.unwrap_or(DEFAULT_MODEL_CALLS);
        if state.spent.model_calls >= max {
            return false;
        }
        state.spent.model_calls = state.spent.model_calls.saturating_add(1);
        true
    }

    /// The calls remaining at this instant, including other host reservations.
    #[must_use]
    pub fn remaining_model_calls(&self) -> u32 {
        let Some(cell) = self.inner.cell.upgrade() else {
            return 0;
        };
        let Ok(state) = cell.state.lock() else {
            return 0;
        };
        state
            .budget
            .max_model_calls
            .unwrap_or(DEFAULT_MODEL_CALLS)
            .saturating_sub(state.spent.model_calls)
    }

    /// The task's remaining active time at this instant, including this host
    /// work interval. `None` means the task has no elapsed-time cap.
    #[must_use]
    pub fn remaining_ms(&self) -> Option<u64> {
        let cell = self.inner.cell.upgrade()?;
        let state = cell.state.lock().ok()?;
        state.budget.max_elapsed_ms.map(|max| {
            max.saturating_sub(state.spent.elapsed_ms).saturating_sub(
                u64::try_from(self.inner.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            )
        })
    }
}

impl fmt::Debug for TaskBudgetScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskBudgetScope")
            .finish_non_exhaustive()
    }
}

struct HostWork {
    cell: Weak<Cell>,
    started: Instant,
    charged: AtomicBool,
}

impl HostWork {
    fn charge(&self) {
        if self.charged.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(cell) = self.cell.upgrade() else {
            return;
        };
        let Ok(mut state) = cell.state.lock() else {
            return;
        };
        state.spent.elapsed_ms = state
            .spent
            .elapsed_ms
            .saturating_add(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX));
    }
}

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
