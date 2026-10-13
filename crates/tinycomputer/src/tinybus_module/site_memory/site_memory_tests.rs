//! A task that names a site is handed what earlier tasks there left, through
//! the host's `SiteMemoryHost`, and leaves what it learned the same way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value
)]

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use tinybus::Connection;

use super::*;

/// A host that keeps sites in memory.
#[derive(Debug, Default)]
struct Store {
    sites: Mutex<BTreeMap<String, SiteMemory>>,
    epoch: Mutex<u64>,
    refuse_saves: Mutex<bool>,
    unreachable: Mutex<bool>,
    saves: Mutex<u32>,
}

#[derive(Debug, Clone, Default)]
struct FakeHost(Arc<Store>);

impl FakeHost {
    fn memory(&self, site: &str) -> SiteMemory {
        self.0
            .sites
            .lock()
            .unwrap()
            .get(site)
            .cloned()
            .unwrap_or_default()
    }

    fn forget(&self, site: &str) {
        self.0.sites.lock().unwrap().remove(site);
        *self.0.epoch.lock().unwrap() += 1;
    }
}

impl SiteMemoryPort for FakeHost {
    fn load<'a>(&'a self, site: &'a str) -> PortFuture<'a, LoadSiteResponse> {
        Box::pin(async move {
            if *self.0.unreachable.lock().unwrap() {
                return Err("no such interface".to_owned());
            }
            Ok(LoadSiteResponse {
                memory: self.memory(site),
                epoch: *self.0.epoch.lock().unwrap(),
            })
        })
    }

    fn save(&self, request: SaveSiteRequest) -> PortFuture<'_, SaveSiteResponse> {
        Box::pin(async move {
            *self.0.saves.lock().unwrap() += 1;
            let stale = request.epoch != *self.0.epoch.lock().unwrap();
            if *self.0.refuse_saves.lock().unwrap() || stale {
                return Ok(SaveSiteResponse { saved: false });
            }
            self.0
                .sites
                .lock()
                .unwrap()
                .insert(request.site, request.memory);
            Ok(SaveSiteResponse { saved: true })
        })
    }
}

fn link(host: &FakeHost) -> SiteLink {
    SiteLink::new(Box::new(host.clone()))
}

fn request(goal: &str) -> StartTaskRequest {
    StartTaskRequest {
        task: Some(goal.to_owned()),
        site: Some("shop.test".to_owned()),
        facts: BTreeMap::from([("name".into(), "Asha Raina".into())]),
        ..StartTaskRequest::default()
    }
}

fn view(id: &str, status: Value) -> TaskView {
    serde_json::from_value(json!({
        "id": id, "status": status, "summary": "", "progress": 1.0, "next": []
    }))
    .unwrap()
}

fn done(id: &str) -> TaskView {
    view(
        id,
        json!({"state": "done", "answer": "ordered", "records": {}}),
    )
}

fn report_of(id: &str, learned: Value, rescues: Value) -> AgentResponse<TaskReport> {
    AgentResponse::ok(
        serde_json::from_value(json!({
            "view": {"id": id, "status": {"state": "done", "answer": "ordered", "records": {}},
                     "summary": "", "progress": 1.0, "next": []},
            "flow": {"app": "browser", "steps": ["open the shop"]},
            "steps": [], "records": {}, "artifacts": [],
            "learned": learned, "trace": [], "rescues": rescues
        }))
        .unwrap(),
    )
}

fn hint(key: &str, name: &str) -> Value {
    json!({"app": "browser", "key": key, "role": "button", "name": name})
}

async fn run_to_the_end(
    link: &SiteLink,
    mut request: StartTaskRequest,
    id: &str,
    report: AgentResponse<TaskReport>,
) {
    let prepared = link.prepare(&mut request).await.unwrap();
    link.follow(&TaskId::new(id), prepared);
    link.learn(&done(id), |_| report).await;
}

#[tokio::test]
async fn a_finished_task_leaves_its_plan_and_elements_for_the_next() {
    let host = FakeHost::default();
    let link = link(&host);
    run_to_the_end(
        &link,
        request("order milk"),
        "t1",
        report_of("t1", json!([hint("k1", "Add to cart")]), json!([])),
    )
    .await;
    let kept = host.memory("shop.test");
    assert_eq!(kept.plans.len(), 1);
    assert_eq!(kept.hints.len(), 1);

    let mut next = request("Order  MILK");
    let prepared = link.prepare(&mut next).await.unwrap();
    assert!(prepared.reuses_plan());
    assert!(next.flow.is_some());
    assert_eq!(next.memory.len(), 1);
}

#[tokio::test]
async fn a_task_without_a_site_or_a_goal_reads_and_writes_nothing() {
    let host = FakeHost::default();
    let link = link(&host);
    let mut no_site = request("order milk");
    no_site.site = None;
    assert!(link.prepare(&mut no_site).await.is_none());
    let mut no_goal = request("order milk");
    no_goal.task = None;
    assert!(link.prepare(&mut no_goal).await.is_none());
    let mut odd_site = request("order milk");
    odd_site.site = Some("../etc".to_owned());
    assert!(link.prepare(&mut odd_site).await.is_none());
    assert_eq!(*host.0.saves.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_host_that_does_not_serve_the_interface_costs_the_task_only_the_memory() {
    let host = FakeHost::default();
    *host.0.unreachable.lock().unwrap() = true;
    let mut request = request("order milk");
    assert!(link(&host).prepare(&mut request).await.is_none());
    assert!(request.flow.is_none());
}

#[tokio::test]
async fn a_caller_flow_is_kept_over_a_saved_plan() {
    let host = FakeHost::default();
    let link = link(&host);
    run_to_the_end(
        &link,
        request("order milk"),
        "t1",
        report_of("t1", json!([]), json!([])),
    )
    .await;
    let mut own = request("order milk");
    let flow: tinycomputer_bus::Flow =
        serde_json::from_value(json!({"app": "browser", "steps": ["my own"]})).unwrap();
    own.flow = Some(flow.clone());
    let prepared = link.prepare(&mut own).await.unwrap();
    assert!(!prepared.reuses_plan());
    assert_eq!(own.flow, Some(flow));
}

#[tokio::test]
async fn a_site_forgotten_while_a_task_runs_is_not_written_back() {
    let host = FakeHost::default();
    let link = link(&host);
    let mut start = request("order milk");
    let prepared = link.prepare(&mut start).await.unwrap();
    link.follow(&TaskId::new("t1"), prepared);
    host.forget("shop.test");
    let report = report_of("t1", json!([hint("k1", "Add to cart")]), json!([]));
    link.learn(&done("t1"), |_| report).await;
    assert!(host.memory("shop.test").is_empty());
}

#[tokio::test]
async fn a_save_the_host_refuses_leaves_the_site_as_it_was() {
    let host = FakeHost::default();
    *host.0.refuse_saves.lock().unwrap() = true;
    run_to_the_end(
        &link(&host),
        request("order milk"),
        "t1",
        report_of("t1", json!([]), json!([])),
    )
    .await;
    assert!(host.memory("shop.test").is_empty());
}

#[tokio::test]
async fn a_reused_plan_that_fails_is_forgotten_and_a_running_task_stays_followed() {
    let host = FakeHost::default();
    let link = link(&host);
    run_to_the_end(
        &link,
        request("order milk"),
        "t1",
        report_of("t1", json!([]), json!([])),
    )
    .await;
    assert_eq!(host.memory("shop.test").plans.len(), 1);

    let mut again = request("order milk");
    let prepared = link.prepare(&mut again).await.unwrap();
    link.follow(&TaskId::new("t2"), prepared);
    link.learn(&view("t2", json!({"state": "running"})), |_| {
        panic!("no report for a running task")
    })
    .await;
    assert_eq!(host.memory("shop.test").plans.len(), 1);
    link.learn(
        &view("t2", json!({"state": "failed", "step": null, "reason": "gone", "hint": "", "recoverable": false})),
        |_| panic!("no report for a failed task"),
    )
    .await;
    assert!(host.memory("shop.test").plans.is_empty());
}

#[tokio::test]
async fn a_refused_plan_is_forgotten_so_the_task_is_planned_afresh() {
    let host = FakeHost::default();
    let link = link(&host);
    run_to_the_end(
        &link,
        request("order milk"),
        "t1",
        report_of("t1", json!([]), json!([])),
    )
    .await;
    let mut again = request("order milk");
    let mut prepared = link.prepare(&mut again).await.unwrap();
    assert!(prepared.reuses_plan());
    link.refused(&mut prepared).await;
    assert!(!prepared.reuses_plan());
    assert!(host.memory("shop.test").plans.is_empty());
}

#[tokio::test]
async fn values_given_to_a_paused_task_are_not_kept() {
    let host = FakeHost::default();
    let link = link(&host);
    let mut start = request("order milk");
    let prepared = link.prepare(&mut start).await.unwrap();
    let id = TaskId::new("t1");
    link.follow(&id, prepared);
    link.note_inputs(&id, &BTreeMap::from([("code".into(), "Zanzibar77".into())]));
    let report = report_of("t1", json!([hint("k1", "Zanzibar77 button")]), json!([]));
    link.learn(&done("t1"), |_| report).await;
    assert!(host.memory("shop.test").hints.is_empty());
}

#[tokio::test]
async fn a_cancelled_task_stops_being_followed_and_teaches_nothing() {
    let host = FakeHost::default();
    let link = link(&host);
    let mut start = request("order milk");
    let prepared = link.prepare(&mut start).await.unwrap();
    link.follow(&TaskId::new("t1"), prepared);
    link.learn(&view("t1", json!({"state": "cancelled"})), |_| {
        panic!("no report")
    })
    .await;
    link.learn(&done("t1"), |_| panic!("no longer followed"))
        .await;
    assert_eq!(*host.0.saves.lock().unwrap(), 0);
}

#[test]
fn only_the_engines_invalid_flow_error_counts_as_a_refused_flow() {
    let refused: AgentResponse<TaskView> = AgentResponse::err(
        tinycomputer_bus::agent::AgentError::new("INVALID_FLOW", "no", "fix", false),
    );
    assert!(refused_flow(&refused));
    let other: AgentResponse<TaskView> = AgentResponse::err(
        tinycomputer_bus::agent::AgentError::new("NO_SUCH_TASK", "INVALID_FLOW", "x", false),
    );
    assert!(!refused_flow(&other));
}

/// A host serving `SiteMemoryHost` over a real bus.
#[derive(Clone)]
struct BusHost(FakeHost);

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.SiteMemoryHost")]
impl BusHost {
    async fn load(&self, request: LoadSiteRequest) -> tinybus::Result<LoadSiteResponse> {
        self.0
            .load(&request.site)
            .await
            .map_err(tinybus::Error::failed)
    }

    async fn save(&self, request: SaveSiteRequest) -> tinybus::Result<SaveSiteResponse> {
        self.0.save(request).await.map_err(tinybus::Error::failed)
    }
}

#[tokio::test]
async fn the_bus_port_loads_and_saves_through_the_hosts_interface() -> tinybus::Result<()> {
    use tinybus::broker::Broker;
    use tinybus::transport::memory::MemoryBus;

    let bus = MemoryBus::new();
    Broker::new().spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    let fake = FakeHost::default();
    host_connection
        .serve_at(OBJECT_PATH.try_into()?, BusHost(fake.clone()))
        .await?;
    host_connection.request_name(INTERFACE).await?;
    let module = Connection::connect(bus.connect().await?).await?;

    let link = SiteLink::new(Box::new(BusPort::new(module)));
    run_to_the_end(
        &link,
        request("order milk"),
        "t1",
        report_of("t1", json!([hint("k1", "Add to cart")]), json!([])),
    )
    .await;
    assert_eq!(fake.memory("shop.test").plans.len(), 1);
    let mut next = request("order milk");
    assert!(link.prepare(&mut next).await.unwrap().reuses_plan());
    Ok(())
}

#[tokio::test]
async fn the_bus_port_reports_a_host_that_serves_nothing() -> tinybus::Result<()> {
    use tinybus::broker::Broker;
    use tinybus::transport::memory::MemoryBus;

    let bus = MemoryBus::new();
    Broker::new().spawn(bus.clone());
    let module = Connection::connect(bus.connect().await?).await?;
    let link = SiteLink::new(Box::new(BusPort::new(module)));
    let mut start = request("order milk");
    assert!(link.prepare(&mut start).await.is_none());
    Ok(())
}
