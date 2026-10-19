use super::*;
use std::sync::Mutex;
use tinycomputer_bus::trace_sink::TraceEvent;

#[derive(Clone, Default)]
struct RecordingHost {
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    artifacts: Arc<Mutex<Vec<TraceArtifact>>>,
    reports: Arc<Mutex<Vec<TaskReportRecord>>>,
    fail: bool,
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.TraceSinkHost")]
impl RecordingHost {
    async fn append(&self, request: TraceEvent) -> TinyBusResult<SinkAck> {
        if self.fail {
            return Err(TinyBusError::failed("sink refused event"));
        }
        self.events.lock().unwrap().push(request.record);
        Ok(SinkAck {})
    }

    async fn store_artifact(&self, request: TraceArtifact) -> TinyBusResult<TraceArtifactRef> {
        if self.fail {
            return Err(TinyBusError::failed("sink refused artifact"));
        }
        self.artifacts.lock().unwrap().push(request.clone());
        Ok(TraceArtifactRef {
            reference: format!("opaque:{}", request.name),
        })
    }

    async fn write_report(&self, request: TaskReportRecord) -> TinyBusResult<SinkAck> {
        if self.fail {
            return Err(TinyBusError::failed("sink refused report"));
        }
        self.reports.lock().unwrap().push(request);
        Ok(SinkAck {})
    }
}

async fn setup_host(
    bus: &tinybus::transport::memory::MemoryBus,
    host: RecordingHost,
) -> TinyBusResult<Connection> {
    let connection = Connection::connect(bus.connect().await?).await?;
    connection
        .serve_at(trace_sink::OBJECT_PATH.try_into()?, host)
        .await?;
    connection.request_name(trace_sink::INTERFACE).await?;
    Ok(connection)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trace_calls_are_ordered_and_artifacts_and_reports_reach_the_host() -> TinyBusResult<()> {
    use tinycomputer_bus::agent::TaskReport;

    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host = RecordingHost::default();
    let _host_connection = setup_host(&bus, host.clone()).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new(module)?;

    let first = serde_json::json!({"event": "first", "seq": 1});
    let second = serde_json::json!({"event": "second", "seq": 2});
    link.append(&first, TraceSinkDeadline::standard().unwrap(), 4096)
        .expect("async executor callback is acknowledged");
    let trace_link = link.clone();
    tokio::task::spawn_blocking(move || {
        trace_link
            .append(&second, TraceSinkDeadline::standard().unwrap(), 4096)
            .unwrap();
        trace_link
            .store_artifact(
                TraceSinkArtifact {
                    kind: EngineArtifactKind::Screenshot,
                    name: "screen.png",
                    bytes: b"image",
                    deadline: TraceSinkDeadline::standard().unwrap(),
                },
                TraceSinkArtifactLimits {
                    screenshot_bytes: 100,
                    screenshot_count: 2,
                    refmap_bytes: 100,
                },
            )
            .unwrap()
    })
    .await
    .expect("trace worker task completes");

    let report = TaskReportRecord {
        task_id: "task-1".into(),
        report: serde_json::from_value::<TaskReport>(serde_json::json!({
            "view": {"id": "task-1"},
            "steps": [], "records": {}, "artifacts": [], "learned": [], "trace": []
        }))
        .unwrap(),
    };
    link.write_report(report).await?;

    assert_eq!(
        *host.events.lock().unwrap(),
        [
            serde_json::json!({"event": "first", "seq": 1}),
            serde_json::json!({"event": "second", "seq": 2})
        ]
    );
    assert_eq!(host.artifacts.lock().unwrap()[0].name, "screen.png");
    assert_eq!(host.reports.lock().unwrap()[0].task_id, "task-1");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sink_failures_are_returned_without_fallback() -> TinyBusResult<()> {
    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let _host_connection = setup_host(
        &bus,
        RecordingHost {
            fail: true,
            ..Default::default()
        },
    )
    .await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new(module)?;

    let trace_link = link.clone();
    let error = tokio::task::spawn_blocking(move || {
        trace_link.append(
            &serde_json::json!({"event": "rejected"}),
            TraceSinkDeadline::standard().unwrap(),
            4096,
        )
    })
    .await
    .expect("trace worker task completes")
    .expect_err("host failure must be returned to strict desktop trace");
    assert!(error.to_string().contains("rejected"));
    assert!(
        link.write_report(TaskReportRecord {
            task_id: "task-1".into(),
            report: serde_json::from_value(serde_json::json!({
                "view": {"id": "task-1"},
                "steps": [], "records": {}, "artifacts": [], "learned": [], "trace": []
            }))
            .unwrap(),
        })
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_report_member_persists_its_report_before_returning() -> TinyBusResult<()> {
    use tinybus::Interface;
    use tinycomputer_bus::agent::{AgentResponse, TaskView};
    use tinycomputer_bus::names;

    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host = RecordingHost::default();
    let _host_connection = setup_host(&bus, host.clone()).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let service = crate::tinybus_module::dispatch::DesktopService::from_config_with_connection(
        &serde_json::json!({"trace_sink": true}),
        module,
    )
    .await?
    .with_native_fixture();

    let started = service
        .call(
            &names::methods::START_TASK.try_into()?,
            serde_json::json!([{"flow": {"app": "Mail", "steps": ["start a note"]}}]),
        )
        .await?;
    let started: AgentResponse<TaskView> = serde_json::from_value(started)?;
    let id = started.data.expect("task starts").id;
    let report = service
        .call(
            &names::methods::TASK_REPORT.try_into()?,
            serde_json::json!([{"id": id, "trace": false}]),
        )
        .await?;

    assert!(report["ok"].as_bool().unwrap_or(false));
    assert_eq!(host.reports.lock().unwrap().len(), 1);
    assert_eq!(host.reports.lock().unwrap()[0].task_id, id.to_string());
    Ok(())
}
