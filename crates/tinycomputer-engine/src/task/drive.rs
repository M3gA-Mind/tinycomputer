//! Running a task in the background: planning it when asked to, running its
//! flows in order, interpreting where each stopped, and finishing it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tinycomputer_bus::agent::{SurfaceKind, TaskConstraints, TaskId, TaskStatus};

use super::artifact::{capture, captured};
use super::budget::{elapsed_budget_failed, run_request, stop_task};
use super::human::human_wall;
use super::interpret::{Next, finished, run_outcome};
use super::publish::{publish, records, stopped_summary};
use super::recovery::{rescue_outcome, rescued};
use super::store::{Cell, Run};
use super::{DEFAULT_VOTES, FlowRunner, SHAPE_TIMEOUT_MS, redact_task_value};
use crate::shape::Harvest;

type TaskRecords = BTreeMap<String, Vec<BTreeMap<String, String>>>;

/// Plans the task, then runs the plan — or asks for what it needs first.
pub(super) async fn plan_then_drive(
    cell: Arc<Cell>,
    runner: Arc<dyn FlowRunner>,
    planner: crate::planner::Planner,
    task: String,
    surfaces: Vec<tinycomputer_bus::agent::SurfaceKind>,
) {
    let (names, secrets, constraints, votes, capped) = cell.state.lock().map_or_else(
        |_| {
            (
                Vec::new(),
                Vec::new(),
                TaskConstraints::default(),
                DEFAULT_VOTES,
                false,
            )
        },
        |state| {
            let owned = |names: Vec<&str>| names.into_iter().map(str::to_owned).collect::<Vec<_>>();
            (
                owned(state.facts.names()),
                owned(state.facts.secret_names()),
                state.constraints.clone(),
                state.budget.votes.unwrap_or(DEFAULT_VOTES),
                state.budget.max_model_calls.is_some(),
            )
        },
    );
    let id = cell.view.borrow().id.clone();
    let usage = task_usage(&cell);
    let planner = planner.with_usage_recorder(Arc::clone(&usage));
    // Jev's connections open while the plan is drafted, so the first
    // decision need not open them; nothing waits for this. A budget that
    // caps the task's Jev calls is not spent on calls it does not count.
    if !capped {
        tokio::spawn(runner.warm(&id, votes));
    }
    let started = Instant::now();
    // Timed on its own: a browser slower to open than the plan is to draft
    // is not planning time.
    let planning = async {
        let drafted = planner
            .plan_measured(&task, &names, &secrets, &surfaces)
            .await;
        (drafted, started.elapsed())
    };
    // A browser-only task's browser opens while the plan is drafted, so the
    // first step need not wait for it: whatever the plan says, it runs there.
    // The one page the task names loads in it meanwhile, for a first step
    // that browses there.
    let ((outcome, used), drafting) = if constraints.surfaces == [SurfaceKind::Browser] {
        let page = super::page::named_page(&task);
        let preparing = async {
            runner.prepare(&id, &constraints).await;
            if let Some(page) = &page {
                runner.open_page(&id, page).await;
            }
        };
        tokio::join!(planning, preparing).0
    } else {
        planning.await
    };
    runner.journal(Some(&id), "plan", &|| {
        super::timing::planned(&outcome, used, drafting, planner.configuration())
    });
    let plan = match outcome {
        Ok(plan) => plan,
        Err(reason) => {
            // A browser opened while planning has nothing left to do; it is
            // let go before the failure is told, as a finished task's is.
            runner.release(&id);
            publish(
                &cell,
                TaskStatus::Failed {
                    step: None,
                    reason: reason.clone(),
                    hint: "reword the task, or pass a flow written with Describe's guide"
                        .to_owned(),
                    recoverable: true,
                },
                &format!("Planning failed: {reason}"),
            );
            return;
        }
    };
    let allow = {
        let Ok(mut state) = cell.state.lock() else {
            return;
        };
        state.flow = plan.flow.clone();
        state.constraints.allow_destructive
    };
    if plan.questions.is_empty() {
        drive(
            cell,
            runner,
            vec![Run {
                flow: plan.flow,
                allow_destructive: allow,
                rescue: None,
            }],
        )
        .await;
    } else {
        // A browser opened while planning is let go while the task waits on
        // a person, who may take long or never answer; the run that follows
        // opens one again.
        runner.release(&id);
        publish(
            &cell,
            TaskStatus::NeedsInput {
                fields: plan.questions,
            },
            "The plan needs values before it can start.",
        );
    }
}

/// Runs a task's flows in order until one stops it or all finish.
///
/// A task's [`TaskBudget`] bounds the whole task, not one run of it: an
/// approval or a human intervention splits a task into several runs, and
/// each is given only what the task has not already spent, so resuming can
/// never reset the budget back to full. `max_elapsed_ms` has no equivalent in
/// [`RunFlowRequest`] — a run cannot police its own wall-clock time from the
/// inside — so it is enforced here instead, by timing out a run that would
/// otherwise run past what remains of it.
pub(super) async fn drive(cell: Arc<Cell>, runner: Arc<dyn FlowRunner>, runs: Vec<Run>) {
    let mut runs = VecDeque::from(runs);
    while let Some(run) = runs.pop_front() {
        let Some((request, constraints, time_left)) = run_request(&cell, &run) else {
            return;
        };
        if time_left == Some(0) {
            stop_task(&cell, runner.as_ref(), elapsed_budget_failed()).await;
            return;
        }
        let id = cell.view.borrow().id.clone();
        let started = Instant::now();
        let run_call = runner.run_with_usage(&id, &constraints, request, task_usage(&cell));
        let reply = if let Some(ms) = time_left {
            let Ok(reply) = tokio::time::timeout(Duration::from_millis(ms), run_call).await else {
                stop_task(&cell, runner.as_ref(), elapsed_budget_failed()).await;
                return;
            };
            reply
        } else {
            run_call.await
        };
        let spent_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (next, result) = run_outcome(&run.flow, &reply, constraints.payment);
        let reached = result
            .as_ref()
            .map_or_else(Vec::new, |result| result.steps.clone());
        let redacted = {
            let Ok(mut state) = cell.state.lock() else {
                return;
            };
            state.spent.elapsed_ms = state.spent.elapsed_ms.saturating_add(spent_ms);
            if let Some(rescue) = run.rescue.and_then(|index| state.rescues.get_mut(index)) {
                rescue.outcome = rescue_outcome(&reached, rescue.steps.len());
            }
            state.dialog_left_open = result
                .as_ref()
                .is_some_and(|result| result.dialog_left_open);
            if let Some(result) = result {
                state.spent.actions = state.spent.actions.saturating_add(result.actions);
                state.spent.model_calls =
                    state.spent.model_calls.saturating_add(result.metrics.calls);
                state.finished += finished(&result.steps);
                state.steps.extend(result.steps);
                state.exchanges.extend(result.trace);
                state.learned.extend(result.learned);
                // A variable the flow declares is the caller's input unless
                // a step writes it: a planner declares each read's variable
                // up front, empty, while one the flow defines from the
                // caller's values (`"recipient": "${email}"`) is expanded as
                // the run starts, so a changed value alone tells nothing.
                let written = written_names(&run.flow.steps);
                for (name, value) in result.vars {
                    let read = match run.flow.vars.get(&name) {
                        None => true,
                        Some(declared) => written.contains(&name) && *declared != value,
                    };
                    if read && state.facts.get(&name).is_none() {
                        state.reads.insert(name, value);
                    }
                }
            }
            if let Next::Stop { resume, .. } = &next {
                state.resume.clone_from(resume);
            }
            state.facts.clone()
        };
        if let Next::Stop { status, .. } = next {
            let status = human_wall(&cell, runner.as_ref(), *status).await;
            let status = match rescued(&cell, runner.as_ref(), &run, status, reached).await {
                Ok(status) => status,
                Err(rescued) => {
                    runs.push_front(rescued);
                    continue;
                }
            };
            let status = captured(&cell, runner.as_ref(), status).await;
            let summary = redacted.redact(&stopped_summary(&status));
            let ended = matches!(
                status,
                TaskStatus::Done { .. } | TaskStatus::Failed { .. } | TaskStatus::Cancelled
            );
            publish(&cell, status, &summary);
            if ended {
                runner.release(&cell.view.borrow().id);
            }
            return;
        }
    }
    finish(&cell, runner.as_ref()).await;
}

/// Every run finished: the task is done, with what it read, shaped as its
/// `output` asks when it asks.
pub(super) async fn finish(cell: &Cell, runner: &dyn FlowRunner) {
    // The last look at the surface, before shaping and before release.
    let _ = capture(cell, runner).await;
    let id = cell.view.borrow().id.clone();
    let Some((answer, records, harvest)) = finish_content(cell, runner, &id) else {
        return;
    };
    let result = match (harvest, cell.shaper.clone()) {
        (Some(harvest), Some(shaper)) => {
            let usage = task_usage(cell);
            let shaper = shaper.with_usage_recorder(usage);
            publish(
                cell,
                TaskStatus::Running,
                "Every step finished; shaping the answer.",
            );
            let answered = tokio::time::timeout(
                Duration::from_millis(SHAPE_TIMEOUT_MS),
                shaper.shape(&harvest),
            )
            .await
            .unwrap_or_else(|_| Err("shaping the answer took too long".to_owned()));
            match answered {
                Ok(result) => Some(result),
                Err(why) => {
                    let steps = cell.state.lock().map_or(0, |state| state.flow.steps.len());
                    let reason =
                        runner.redact(&id, &format!("the answer could not be shaped: {why}"));
                    publish(
                        cell,
                        TaskStatus::Failed {
                            step: Some(steps),
                            reason: reason.clone(),
                            hint: "every step finished; TaskReport holds the records they read"
                                .to_owned(),
                            recoverable: false,
                        },
                        &format!("The task failed: {reason}"),
                    );
                    runner.release(&cell.view.borrow().id);
                    return;
                }
            }
        }
        _ => None,
    };
    publish(
        cell,
        TaskStatus::Done {
            answer: answer.clone(),
            records,
            result: result.map(|mut value| {
                redact_task_value(runner, &id, &mut value);
                value
            }),
        },
        &answer,
    );
    runner.release(&cell.view.borrow().id);
}

fn finish_content(
    cell: &Cell,
    runner: &dyn FlowRunner,
    id: &TaskId,
) -> Option<(String, TaskRecords, Option<Harvest>)> {
    let state = cell.state.lock().ok()?;
    let reads = state
        .reads
        .iter()
        .map(|(name, value)| {
            let name = runner.redact(id, name);
            let value = runner.redact(id, value);
            format!("{name}: {value}")
        })
        .collect::<Vec<_>>();
    let answer = if reads.is_empty() {
        format!("Finished all {} steps.", state.flow.steps.len())
    } else {
        format!(
            "Finished all {} steps. {}",
            state.flow.steps.len(),
            reads.join("; ")
        )
    };
    let harvest = state.output.clone().map(|mut output| {
        output.instructions = runner.redact(id, &output.instructions);
        if let Some(schema) = &mut output.schema {
            redact_task_value(runner, id, schema);
        }
        Harvest {
            goal: runner.redact(id, &state.facts.redact(&state.goal)),
            output,
            reads: state
                .reads
                .iter()
                .map(|(name, value)| {
                    (
                        runner.redact(id, name),
                        runner.redact(id, &state.facts.redact(value)),
                    )
                })
                .collect(),
        }
    });
    let reads = state
        .reads
        .iter()
        .map(|(name, value)| (runner.redact(id, name), runner.redact(id, value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    Some((
        runner.redact(id, &state.facts.redact(&answer)),
        records(&reads),
        harvest,
    ))
}

fn task_usage(cell: &Cell) -> Arc<crate::UsageRecorder> {
    cell.state
        .lock()
        .map(|state| Arc::clone(&state.usage))
        .unwrap_or_default()
}

/// Every variable a step of `steps` writes: a `read`'s, `extract`'s, or
/// `pick`'s `into`, at any depth.
fn written_names(steps: &[tinycomputer_bus::FlowStep]) -> std::collections::BTreeSet<String> {
    use tinycomputer_bus::FlowAction;
    let mut names = std::collections::BTreeSet::new();
    for step in steps {
        match step.action() {
            FlowAction::Read(read) | FlowAction::Extract(read) => {
                names.insert(read.into);
            }
            FlowAction::Pick(pick) => names.extend(pick.into),
            FlowAction::If(branch) => {
                names.extend(written_names(&branch.then));
                names.extend(written_names(&branch.otherwise));
            }
            FlowAction::RepeatUntil(repeat) => names.extend(written_names(&repeat.steps)),
            _ => {}
        }
    }
    names
}
