//! Versioned snapshots for tasks paused for input, approval, or human help.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tinycomputer_bus::Flow;
use tinycomputer_bus::agent::{TaskId, TaskStatus, TaskView};
use tinycomputer_core::{Facts, SecretFactHandle};

use super::interpret::Resume;
use super::publish::{needs_input, publish};
use super::store::{Cell, Spent, State};
use super::{MAX_TASKS, TaskCallBudget, Tasks};
use crate::rescue::Rescuer;
use crate::{UsageRecorder, shape::Shaper};

const VERSION: u32 = 2;
const OLDEST_VERSION: u32 = 1;

mod error;
mod export;
mod import;
mod types;

pub use error::PausedTaskError;
pub(super) use types::{Envelope, PausedTask};
