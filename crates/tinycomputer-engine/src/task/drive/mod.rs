//! Running a task in the background: planning it when asked to, running its
//! flows in order, interpreting where each stopped, and finishing it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use tinycomputer_bus::FLOW_GUIDE;
use tinycomputer_bus::agent::{SurfaceKind, TaskConstraints, TaskId, TaskStatus};

use super::artifact::{capture, captured};
use super::budget::{elapsed_budget_failed, run_request, stop_task};
use super::human::human_wall;
use super::interpret::{Next, finished, run_outcome};
use super::publish::{publish, records, stopped_summary};
use super::recovery::{rescue_outcome, rescued};
use super::store::{Cell, PaymentCheckpointState, Run};
use super::{DEFAULT_VOTES, FlowRunner, SHAPE_TIMEOUT_MS, redact_task_value};
use crate::shape::Harvest;

type TaskRecords = BTreeMap<String, Vec<BTreeMap<String, String>>>;

mod finish;
mod handoff;
mod plan;
mod run;

pub(super) use finish::finish;
pub(super) use plan::{plan_payment_then_drive, plan_then_drive};
pub(super) use run::drive;
