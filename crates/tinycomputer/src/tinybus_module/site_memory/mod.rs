//! What a browser task learns about its site, kept through the host's
//! `SiteMemoryHost` interface.
//!
//! The module keeps nothing between tasks itself. A `StartTask` that names a
//! `site` asks the host for what finished tasks there left (`Load`), hands the
//! task a saved plan for the same goal and facts and the elements earlier
//! tasks found, and follows the task. Once it ends, the module reads the
//! report, applies the rules in [`tinycomputer_core::sites`] and gives the
//! result back (`Save`). The host decides only where the bytes live.
//!
//! A host that does not serve the interface, or answers an error, costs the
//! task nothing but the memory: it runs as if the site were new.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tinybus::Connection;
use tinycomputer_bus::agent::{
    AgentResponse, StartTaskRequest, TaskId, TaskReport, TaskStatus, TaskView,
};
use tinycomputer_bus::site_memory::{
    INTERFACE, LoadSiteRequest, LoadSiteResponse, OBJECT_PATH, SaveSiteRequest, SaveSiteResponse,
    SiteMemory, methods,
};
use tinycomputer_core::sites::{self, Following, SiteMemoryRules};

/// How many started tasks are followed at once; the oldest is let go first.
const FOLLOWED: usize = 64;
/// The deadline for one call to the host.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A pending call to the host.
pub(crate) type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// The host's `SiteMemoryHost`, as the module calls it.
pub(crate) trait SiteMemoryPort: Send + Sync + std::fmt::Debug {
    /// `Load`: one site's memory.
    fn load<'a>(&'a self, site: &'a str) -> PortFuture<'a, LoadSiteResponse>;
    /// `Save`: replaces one site's memory.
    fn save(&self, request: SaveSiteRequest) -> PortFuture<'_, SaveSiteResponse>;
}

/// The port over the bus connection the module was set up on.
#[derive(Clone)]
pub(crate) struct BusPort {
    connection: Connection,
}

impl std::fmt::Debug for BusPort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("BusPort").finish_non_exhaustive()
    }
}

impl BusPort {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    fn proxy(&self) -> Result<tinybus::Proxy, String> {
        self.connection
            .proxy(INTERFACE, OBJECT_PATH, INTERFACE)
            .map(|proxy| proxy.with_timeout(CALL_TIMEOUT))
            .map_err(|error| error.to_string())
    }
}

impl SiteMemoryPort for BusPort {
    fn load<'a>(&'a self, site: &'a str) -> PortFuture<'a, LoadSiteResponse> {
        Box::pin(async move {
            let request = LoadSiteRequest {
                site: site.to_owned(),
            };
            self.proxy()?
                .call(methods::LOAD, (request,))
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn save(&self, request: SaveSiteRequest) -> PortFuture<'_, SaveSiteResponse> {
        Box::pin(async move {
            self.proxy()?
                .call(methods::SAVE, (request,))
                .await
                .map_err(|error| error.to_string())
        })
    }
}

/// A task about to start, with the memory it was handed.
#[derive(Debug)]
pub(crate) struct Prepared {
    following: Following,
    memory: SiteMemory,
    epoch: u64,
}

impl Prepared {
    /// Whether the task runs a plan saved for its goal and facts.
    pub(crate) fn reuses_plan(&self) -> bool {
        self.following.reuses_plan()
    }
}

/// A followed task: what it may teach, and the epoch it started under.
#[derive(Debug)]
struct Followed {
    following: Following,
    epoch: u64,
}

/// The module's link to the host's site memory, and the tasks it follows.
#[derive(Debug)]
pub(crate) struct SiteLink {
    port: Box<dyn SiteMemoryPort>,
    followed: Mutex<VecDeque<(TaskId, Followed)>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl SiteLink {
    pub(crate) fn new(port: Box<dyn SiteMemoryPort>) -> Self {
        Self {
            port,
            followed: Mutex::new(VecDeque::new()),
        }
    }

    fn followed(&self) -> std::sync::MutexGuard<'_, VecDeque<(TaskId, Followed)>> {
        self.followed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Fills `request` from what finished tasks on its `site` left: the plan
    /// saved for its goal and facts, unless the caller brought a flow of its
    /// own, and the elements found there. `None` when the request names no
    /// site or goal, or the host has nothing to say.
    pub(crate) async fn prepare(&self, request: &mut StartTaskRequest) -> Option<Prepared> {
        let site = sites::site_named(request.site.as_deref()?)?;
        let goal = request.task.as_deref()?;
        let Ok(loaded) = self.port.load(&site).await else {
            return None;
        };
        let mut memory = loaded.memory;
        memory.expire(now());
        let mut following = Following::new(site, goal, &request.facts, false);
        if request.flow.is_none()
            && let Some(plan) = memory.plan_for(following.goal(), following.facts_id())
        {
            request.flow = Some(plan.clone());
            following = Following::new(following.site().to_owned(), goal, &request.facts, true);
        }
        if request.memory.is_empty() {
            request.memory = memory
                .hints
                .iter()
                .map(|saved| saved.hint.clone())
                .collect();
        }
        Some(Prepared {
            following,
            memory,
            epoch: loaded.epoch,
        })
    }

    /// Forgets the saved plan the module refused, so the task is planned
    /// afresh. When it cannot be saved, the task still counts as reusing it,
    /// and a failure forgets it then.
    pub(crate) async fn refused(&self, prepared: &mut Prepared) {
        prepared.memory.forget_plan(&prepared.following);
        if self
            .save(&prepared.following, &mut prepared.memory, prepared.epoch)
            .await
        {
            prepared.following.plan_refused();
        }
    }

    /// Follows task `id` until it ends, to learn from how it did.
    pub(crate) fn follow(&self, id: &TaskId, prepared: Prepared) {
        let mut followed = self.followed();
        followed.retain(|(known, _)| known != id);
        followed.push_back((
            id.clone(),
            Followed {
                following: prepared.following,
                epoch: prepared.epoch,
            },
        ));
        while followed.len() > FOLLOWED {
            followed.pop_front();
        }
    }

    /// Adds `inputs`, the values a paused task `id` is given, to the values
    /// it may not keep.
    pub(crate) fn note_inputs(&self, id: &TaskId, inputs: &BTreeMap<String, String>) {
        if inputs.is_empty() {
            return;
        }
        if let Some((_, followed)) = self.followed().iter_mut().find(|(known, _)| known == id) {
            followed.following.note_inputs(inputs.values());
        }
    }

    /// Learns from a followed task that has ended: what it found and the plan
    /// it ran when it finished, or that the plan it reused failed. `report`
    /// fetches the task's record. A task still running or paused stays
    /// followed.
    pub(crate) async fn learn(
        &self,
        view: &TaskView,
        report: impl FnOnce(&TaskId) -> AgentResponse<TaskReport>,
    ) {
        match &view.status {
            TaskStatus::Done { .. } | TaskStatus::Checkpoint { .. } => {
                let Some(followed) = self.stop(&view.id) else {
                    return;
                };
                let Some(report) = report(&view.id).data else {
                    return;
                };
                self.update(&followed, |memory, following| {
                    memory.learn(following, &report, now());
                })
                .await;
            }
            TaskStatus::Failed { .. } => {
                let Some(followed) = self.stop(&view.id) else {
                    return;
                };
                if followed.following.reuses_plan() {
                    self.update(&followed, SiteMemory::forget_plan).await;
                }
            }
            TaskStatus::Cancelled | TaskStatus::NeedsPlan { .. } => {
                self.stop(&view.id);
            }
            _ => {}
        }
    }

    fn stop(&self, id: &TaskId) -> Option<Followed> {
        let mut followed = self.followed();
        let index = followed.iter().position(|(known, _)| known == id)?;
        followed.remove(index).map(|(_, followed)| followed)
    }

    /// Loads the site's current memory, applies `change`, and saves it, unless
    /// the site was forgotten since the task started.
    async fn update(&self, followed: &Followed, change: impl FnOnce(&mut SiteMemory, &Following)) {
        let Ok(loaded) = self.port.load(followed.following.site()).await else {
            return;
        };
        if loaded.epoch != followed.epoch {
            return;
        }
        let mut memory = loaded.memory;
        memory.expire(now());
        change(&mut memory, &followed.following);
        self.save(&followed.following, &mut memory, followed.epoch)
            .await;
    }

    async fn save(&self, following: &Following, memory: &mut SiteMemory, epoch: u64) -> bool {
        if memory.fit().is_err() {
            return false;
        }
        let request = SaveSiteRequest {
            site: following.site().to_owned(),
            memory: std::mem::take(memory),
            epoch,
        };
        let kept = request.memory.clone();
        let saved = self.port.save(request).await.is_ok_and(|reply| reply.saved);
        *memory = kept;
        saved
    }
}

/// Whether `reply` is the module refusing the flow a task was handed.
pub(crate) fn refused_flow(reply: &AgentResponse<TaskView>) -> bool {
    reply
        .error
        .as_ref()
        .is_some_and(|error| error.code == "INVALID_FLOW")
}

#[cfg(test)]
mod site_memory_tests;
