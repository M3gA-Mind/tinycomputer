//! The versioned snapshot envelope and saved task state.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    version: u32,
    task: PausedTask,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PausedTask {
    id: TaskId,
    view: TaskView,
    flow: Flow,
    #[serde(default)]
    handoff_to_planner: bool,
    goal: String,
    shared_facts: BTreeMap<String, String>,
    secret_handles: BTreeMap<String, tinycomputer_bus::agent::SecretHandle>,
    omitted_secrets: BTreeSet<String>,
    constraints: TaskConstraints,
    budget: tinycomputer_bus::agent::TaskBudget,
    memory: Vec<GroundingHint>,
    trace: bool,
    steps: Vec<StepReport>,
    exchanges: Vec<JevExchange>,
    learned: Vec<GroundingHint>,
    reads: BTreeMap<String, String>,
    dialog_left_open: bool,
    finished: usize,
    resume: Option<Resume>,
    return_after_input: Option<TaskStatus>,
    spent: Spent,
    rescues: Vec<Rescue>,
    output: Option<tinycomputer_bus::agent::TaskOutput>,
    artifacts: Vec<tinycomputer_bus::browser::OutputRef>,
    usage: TaskUsage,
}

