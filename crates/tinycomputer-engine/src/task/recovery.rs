//! Rescuing a failed step: handing it to the task's rescuer with what the
//! run reached, recording the rescue, and turning its guidance into a run.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use tinycomputer_bus::agent::{Rescue, RescueOutcome, TaskId, TaskStatus};
use tinycomputer_bus::{Flow, FlowStep, StepOutcome, StepReport};
use tinycomputer_core::Facts;

use super::brief::brief;
use super::names::{fact_names, known_names};
use super::publish::publish;
use super::store::{Cell, Run};
use super::timing;
use super::{FlowRunner, RESCUE_TIMEOUT_MS};
use crate::rescue::{Briefing, Guidance, MAX_RESCUES, Rescuer, resumed};

/// A recoverable failure of a top-level step is first rescued: `Err` holds
/// the run the guidance makes, to run next. Anything else, or a rescue that
/// gave up or was not tried, is the status to publish.
pub(super) async fn rescued(
    cell: &Cell,
    runner: &dyn FlowRunner,
    run: &Run,
    status: TaskStatus,
    reached: Vec<StepReport>,
) -> Result<TaskStatus, Run> {
    let TaskStatus::Failed {
        step: Some(failed),
        reason,
        hint,
        recoverable: true,
    } = status
    else {
        return Ok(status);
    };
    let hint = match rescue(cell, runner, run, failed, &reason, reached).await {
        Rescued::Run(rescued) => return Err(rescued),
        Rescued::GaveUp(why) => {
            format!("the rescuer gave up: {why}; reword the step, split it, or take over")
        }
        Rescued::Skipped => hint,
    };
    Ok(TaskStatus::Failed {
        step: Some(failed),
        reason,
        hint,
        recoverable: true,
    })
}

/// What a rescue came to.
pub(super) enum Rescued {
    /// Run this next: the guidance, then the rest of the failed run.
    Run(Run),
    /// The rescuer gave up, failed, or gave no valid guidance, for this
    /// reason.
    GaveUp(String),
    /// No rescue was tried: none is configured, or the task's are spent.
    Skipped,
}

/// The names a rescue's steps may use — the flow's, the facts', and what the
/// task saved — and what it saved, fact values redacted.
pub(super) fn recalled(
    flow: &Flow,
    facts: &Facts,
    reads: &BTreeMap<String, String>,
) -> (BTreeSet<String>, Vec<(String, String)>) {
    let known = known_names(flow, facts)
        .into_iter()
        .chain(reads.keys().cloned())
        .collect();
    let collected = reads
        .iter()
        .map(|(name, value)| (name.clone(), facts.redact(&facts.mask(value))))
        .collect();
    (known, collected)
}

/// `step` with every fact value in its text and note redacted.
pub(super) fn redacted(facts: &Facts, mut step: StepReport) -> StepReport {
    step.text = facts.redact(&step.text);
    step.note = facts.redact(&step.note);
    step
}

/// Hands the step `failed` of `run`, which failed for `failure`, to the
/// task's rescuer, with what the run reached and the screen now — every
/// fact value redacted. Returns the run its guidance makes, or why not.
pub(super) async fn rescue(
    cell: &Cell,
    runner: &dyn FlowRunner,
    run: &Run,
    failed: usize,
    failure: &str,
    reached: Vec<StepReport>,
) -> Rescued {
    let Some(rescuer) = cell.rescuer.clone() else {
        return Rescued::Skipped;
    };
    let usage = cell
        .state
        .lock()
        .map(|state| std::sync::Arc::clone(&state.usage))
        .unwrap_or_default();
    let rescuer = rescuer.with_usage_recorder(usage);
    let (facts, goal, earlier, limit, time_left, brief_rules, reads) = {
        let Ok(state) = cell.state.lock() else {
            return Rescued::Skipped;
        };
        let limit = state
            .budget
            .max_rescues
            .unwrap_or(MAX_RESCUES)
            .min(MAX_RESCUES);
        let used = u32::try_from(state.rescues.len()).unwrap_or(u32::MAX);
        if used >= limit {
            return Rescued::Skipped;
        }
        let time_left = state
            .budget
            .max_elapsed_ms
            .map(|max| max.saturating_sub(state.spent.elapsed_ms));
        (
            state.facts.clone(),
            state.goal.clone(),
            state.rescues.clone(),
            limit,
            time_left,
            brief(&state).rules,
            state.reads.clone(),
        )
    };
    let attempt = earlier.len() + 1;
    let id = cell.view.borrow().id.clone();
    let screen = runner
        .visible_text(&id)
        .await
        .iter()
        .map(|line| facts.redact(&facts.mask(line)))
        .collect();
    let (known, collected) = recalled(&run.flow, &facts, &reads);
    let briefing = Briefing {
        goal: facts.redact(&goal),
        flow: run.flow.clone(),
        failed,
        failure: facts.redact(failure),
        steps: reached
            .into_iter()
            .map(|step| redacted(&facts, step))
            .collect(),
        earlier,
        screen,
        rules: brief_rules,
        known,
        collected,
        secrets: fact_names(&facts),
    };
    publish(
        cell,
        TaskStatus::Running,
        &format!(
            "Step {} failed; asking for guidance (rescue {attempt} of {limit}).",
            failed + 1
        ),
    );
    let wait = time_left.map_or(RESCUE_TIMEOUT_MS, |left| left.min(RESCUE_TIMEOUT_MS));
    let (record, guided, spent_ms) =
        ask(runner, &id, &rescuer, &briefing, wait, (attempt, limit)).await;
    let guided = guided.map(|steps| resumed(&briefing, steps, record.covers));
    let reason = facts.redact(&record.reason);
    let index = {
        let Ok(mut state) = cell.state.lock() else {
            return Rescued::Skipped;
        };
        state.spent.elapsed_ms = state.spent.elapsed_ms.saturating_add(spent_ms);
        state.rescues.push(record);
        state.rescues.len() - 1
    };
    let Some(flow) = guided else {
        return Rescued::GaveUp(reason);
    };
    publish(
        cell,
        TaskStatus::Running,
        &format!("Rescue {attempt} of {limit}: {reason}"),
    );
    Rescued::Run(Run {
        flow,
        allow_destructive: run.allow_destructive,
        rescue: Some(index),
    })
}

/// Asks `rescuer` about `briefing`, waiting at most `wait` ms, journals how
/// it went as rescue `attempt` of `limit`, and records it: the record, the
/// guidance's steps when it gave any, and the milliseconds asking took.
async fn ask(
    runner: &dyn FlowRunner,
    id: &TaskId,
    rescuer: &Rescuer,
    briefing: &Briefing,
    wait: u64,
    (attempt, limit): (usize, u32),
) -> (Rescue, Option<Vec<FlowStep>>, u64) {
    let started = Instant::now();
    let (answer, used) = match tokio::time::timeout(
        Duration::from_millis(wait),
        rescuer.guide_measured(briefing),
    )
    .await
    {
        Ok((answer, used)) => (answer, Some(used)),
        Err(_) => (Err("the rescuer took too long".to_owned()), None),
    };
    let took = started.elapsed();
    let outcome = timing::answered(&answer, used.is_none());
    let (record, guided) = record(briefing.failed, briefing.failure.clone(), answer);
    runner.journal(Some(id), "rescue", &|| {
        timing::rescued(&timing::Rescued {
            attempt,
            limit,
            took,
            used,
            outcome,
            record: &record,
            model: rescuer.configuration(),
        })
    });
    let spent_ms = u64::try_from(took.as_millis()).unwrap_or(u64::MAX);
    (record, guided, spent_ms)
}

/// The record of a rescue of step `failed`, and the guidance's steps when
/// it gave any.
pub(super) fn record(
    failed: usize,
    failure: String,
    answer: Result<Guidance, String>,
) -> (Rescue, Option<Vec<FlowStep>>) {
    let (reason, steps, covers, outcome) = match answer {
        Ok(Guidance::Retry {
            reason,
            steps,
            covers,
        }) => (reason, steps, covers, RescueOutcome::Running),
        Ok(Guidance::GiveUp { reason }) | Err(reason) => {
            (reason, Vec::new(), 0, RescueOutcome::GaveUp)
        }
    };
    let guided = (outcome == RescueOutcome::Running).then(|| steps.clone());
    (
        Rescue {
            step: failed,
            failure,
            reason,
            steps,
            covers,
            outcome,
        },
        guided,
    )
}

/// How a rescue went, from the steps its run reached: recovered when each
/// of the `count` guidance steps (the run's first top-level steps) finished
/// or reached its approval.
pub(super) fn rescue_outcome(reached: &[StepReport], count: usize) -> RescueOutcome {
    let finished = (1..=count).all(|number| {
        reached.iter().any(|step| {
            step.path == number.to_string()
                && matches!(
                    step.outcome,
                    StepOutcome::Done | StepOutcome::AlreadyDone | StepOutcome::Gated
                )
        })
    });
    if finished {
        RescueOutcome::Recovered
    } else {
        RescueOutcome::FailedAgain
    }
}
