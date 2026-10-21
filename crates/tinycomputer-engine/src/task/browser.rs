//! A browser-only [`super::FlowRunner`] for hosts embedding the engine.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tinycomputer_browser::{ActionContext, Browser, BrowserSurface, SessionOptions};
use tinycomputer_bus::agent::{PaymentMode, TaskConstraints, TaskId};
use tinycomputer_bus::browser::{ScreenshotRequest, SessionId};
use tinycomputer_bus::{DesktopError, DesktopResponse, RunFlowRequest};
use tinycomputer_core::surface::{Depth, Surface};

use super::{CaptureFuture, FlowFuture, FlowRunner, PrepareFuture, TextFuture};
use crate::SecretHost;
use crate::{JevRuntime, UsageRecorder};

/// Runs task flows directly on one in-process browser surface per task.
///
/// The host owns the browser launcher and the configured Jev runtime. This
/// runner adds no bus or desktop adapter. Each task keeps its browser session
/// across pauses and continuations, and [`FlowRunner::release`] closes it when
/// the task ends. When a `SecretHost` is configured, tasks have no screenshot
/// artifacts, since host-held values rendered by a page cannot be scrubbed
/// from pixels.
pub struct BrowserRunner {
    browser: Arc<Browser>,
    runtime: JevRuntime,
    options: SessionOptions,
    surfaces: Mutex<HashMap<TaskId, BrowserSurface>>,
    secret_host: Option<Arc<dyn SecretHost>>,
}

impl std::fmt::Debug for BrowserRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BrowserRunner")
            .field("runtime", &self.runtime)
            .field("options", &self.options)
            .field("surfaces", &"[task surfaces]")
            .finish_non_exhaustive()
    }
}

impl BrowserRunner {
    /// Creates a runner using `browser` and the host's configured `runtime`.
    #[must_use]
    pub fn new(browser: Arc<Browser>, runtime: JevRuntime) -> Self {
        Self {
            browser,
            runtime,
            options: SessionOptions::default(),
            surfaces: Mutex::new(HashMap::new()),
            secret_host: None,
        }
    }

    /// Uses `host` to resolve opaque secret handles and type them at the
    /// moment a task fills a field.
    #[must_use]
    pub fn with_secret_host(mut self, host: Arc<dyn SecretHost>) -> Self {
        self.secret_host = Some(host);
        self
    }

    /// The browser session this task runner opened, once it is prepared.
    #[must_use]
    pub fn session_id(&self, task: &TaskId) -> Option<SessionId> {
        self.surfaces.lock().ok()?.get(task)?.session()
    }

    /// Uses `options` as the host defaults for sessions this runner opens.
    #[must_use]
    pub fn with_session_options(mut self, options: SessionOptions) -> Self {
        self.options = options;
        self
    }

    fn surface(&self, task: &TaskId, constraints: &TaskConstraints) -> BrowserSurface {
        let fresh = || {
            let mut options = self.options.clone();
            if constraints.browser_endpoint.is_some() {
                options.endpoint.clone_from(&constraints.browser_endpoint);
            }
            if constraints.browser_executable.is_some() {
                options
                    .executable
                    .clone_from(&constraints.browser_executable);
            }
            if constraints.browser_profile.is_some() {
                options
                    .user_data_dir
                    .clone_from(&constraints.browser_profile);
            }
            if !constraints.origins.is_empty() {
                options.allowed_origins.clone_from(&constraints.origins);
            }
            options.headless = !constraints.headed;
            BrowserSurface::new(
                self.browser.clone(),
                options,
                tokio::runtime::Handle::current(),
            )
            .with_action_context(ActionContext {
                task_id: Some(task.0.clone()),
                session_id: None,
                payment_in_play: constraints.payment == PaymentMode::FillThenApprove,
            })
        };

        self.surfaces.lock().map_or_else(
            |_| fresh(),
            |mut surfaces| surfaces.entry(task.clone()).or_insert_with(fresh).clone(),
        )
    }
}

impl FlowRunner for BrowserRunner {
    fn run(
        &self,
        task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
    ) -> FlowFuture {
        self.run_with_runtime(task, constraints, request, &self.runtime)
    }

    fn page_origin(&self, task: &TaskId) -> super::PageOriginFuture {
        let Some(session) = self.session_id(task) else {
            return Box::pin(async { None });
        };
        let browser = self.browser.clone();
        Box::pin(async move {
            let url = browser
                .list_sessions()
                .await
                .ok()?
                .into_iter()
                .find(|info| info.id == session)?
                .url;
            let url = url::Url::parse(&url).ok()?;
            if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
                return None;
            }
            Some(format!("https://{}/", url.host_str()?))
        })
    }

    fn run_with_usage(
        &self,
        task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
        usage: Arc<UsageRecorder>,
    ) -> FlowFuture {
        let runtime = self.runtime.clone().with_usage_recorder(usage);
        self.run_with_runtime(task, constraints, request, &runtime)
    }

    fn visible_text(&self, task: &TaskId) -> TextFuture {
        let surface = self.surface(task, &TaskConstraints::default());
        let host = self.secret_host.clone();
        let task_id = task.0.clone();
        Box::pin(async move {
            let text: Vec<String> = tokio::task::spawn_blocking(move || {
                surface
                    .observe("browser", None, Depth::Full)
                    .map(|screen| {
                        screen
                            .context
                            .into_iter()
                            .chain(screen.candidates.iter().filter_map(|node| {
                                let held = node
                                    .value
                                    .as_ref()
                                    .and_then(serde_json::Value::as_str)
                                    .filter(|held| !held.trim().is_empty());
                                match (&node.name, held) {
                                    (Some(name), Some(held)) => Some(format!("{name} = {held:?}")),
                                    (name, _) => name.clone(),
                                }
                            }))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            match host {
                Some(host) => text
                    .into_iter()
                    .map(|line| host.redact(&task_id, &line))
                    .collect(),
                None => text,
            }
        })
    }

    fn capture(&self, task: &TaskId) -> CaptureFuture {
        if self.secret_host.is_some() {
            return Box::pin(async { None });
        }
        let surface = self
            .surfaces
            .lock()
            .ok()
            .and_then(|surfaces| surfaces.get(task).cloned());
        let Some(session) = surface.and_then(|surface| surface.session()) else {
            return Box::pin(async { None });
        };
        let browser = self.browser.clone();
        Box::pin(async move {
            browser
                .screenshot(&session, ScreenshotRequest::default())
                .await
                .ok()
        })
    }

    fn prepare(&self, task: &TaskId, constraints: &TaskConstraints) -> PrepareFuture {
        let surface = self.surface(task, constraints);
        Box::pin(async move {
            let _opened = tokio::task::spawn_blocking(move || surface.open()).await;
        })
    }

    fn open_page(&self, task: &TaskId, url: &str) -> PrepareFuture {
        let surface = self
            .surfaces
            .lock()
            .ok()
            .and_then(|surfaces| surfaces.get(task).cloned());
        let Some(surface) = surface else {
            return Box::pin(async {});
        };
        let url = url.to_owned();
        Box::pin(async move {
            let _loaded = tokio::task::spawn_blocking(move || surface.open_at(&url)).await;
        })
    }

    fn warm(&self, task: &TaskId, votes: u32) -> PrepareFuture {
        let runtime = self.runtime.journaled_as(&format!("task-{task}"));
        Box::pin(async move { runtime.warm(votes).await })
    }

    fn warm_with_usage(
        &self,
        task: &TaskId,
        votes: u32,
        usage: Arc<crate::UsageRecorder>,
    ) -> PrepareFuture {
        let runtime = self
            .runtime
            .journaled_as(&format!("task-{task}"))
            .with_usage_recorder(usage);
        Box::pin(async move { runtime.warm(votes).await })
    }

    fn journal(&self, task: Option<&TaskId>, event: &str, fields: &dyn Fn() -> serde_json::Value) {
        if !self.runtime.journaling() {
            return;
        }
        let runtime = task.map_or_else(
            || self.runtime.clone(),
            |task| self.runtime.journaled_as(&format!("task-{task}")),
        );
        runtime.journal_event(event, fields);
    }

    fn release(&self, task: &TaskId) {
        if let Some(surface) = self
            .surfaces
            .lock()
            .ok()
            .and_then(|mut surfaces| surfaces.remove(task))
        {
            surface.close();
        }
    }

    fn redact(&self, task: &TaskId, text: &str) -> String {
        self.secret_host
            .as_ref()
            .map_or_else(|| text.to_owned(), |host| host.redact(&task.0, text))
    }
}

impl BrowserRunner {
    fn run_with_runtime(
        &self,
        task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
        runtime: &JevRuntime,
    ) -> FlowFuture {
        let runtime = runtime.journaled_as(&format!("task-{task}"));
        let surface = self.surface(task, constraints);
        let context = ActionContext {
            task_id: Some(task.0.clone()),
            session_id: None,
            payment_in_play: constraints.payment == PaymentMode::FillThenApprove,
        };
        let mut request = request;
        request.task_id = Some(task.0.clone());
        let secret_host = self.secret_host.clone();
        Box::pin(async move {
            if surface.update_action_context(context).await.is_err() {
                return DesktopResponse::err(
                    "update-action-context",
                    DesktopError::new(
                        "ACTION_CONTEXT_FAILED",
                        "the retained browser session could not update its host payment context",
                    ),
                );
            }
            tinycomputer_engine_flow(surface, runtime, request, secret_host).await
        })
    }
}

async fn tinycomputer_engine_flow(
    surface: BrowserSurface,
    runtime: JevRuntime,
    request: RunFlowRequest,
    secret_host: Option<Arc<dyn SecretHost>>,
) -> DesktopResponse {
    crate::run_flow_with_host(surface, &runtime, request, secret_host).await
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod browser_tests;
