use super::*;
use std::sync::Mutex;
use tinycomputer_bus::agent::TaskReport;
use tinycomputer_bus::trace_sink::{TaskReportRecord, TraceEvent};

#[derive(Clone, Default)]
struct RecordingHost(Arc<Mutex<Vec<serde_json::Value>>>);

#[derive(Clone, Default)]
struct CompleteRecordingHost {
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    artifacts: Arc<Mutex<Vec<TraceArtifact>>>,
    reports: Arc<Mutex<Vec<TaskReportRecord>>>,
    reject: Arc<AtomicBool>,
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.TraceSinkHost")]
impl CompleteRecordingHost {
    async fn append(&self, event: TraceEvent) -> TinyBusResult<SinkAck> {
        tokio::task::yield_now().await;
        if self.reject.load(Ordering::Acquire) {
            return Err(TinyBusError::failed("host rejected event"));
        }
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.record);
        Ok(SinkAck {})
    }

    async fn store_artifact(&self, artifact: TraceArtifact) -> TinyBusResult<TraceArtifactRef> {
        tokio::task::yield_now().await;
        if self.reject.load(Ordering::Acquire) {
            return Err(TinyBusError::failed("host rejected artifact"));
        }
        self.artifacts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(artifact);
        Ok(TraceArtifactRef {
            reference: "trace-artifact-1".into(),
        })
    }

    async fn write_report(&self, report: TaskReportRecord) -> TinyBusResult<SinkAck> {
        tokio::task::yield_now().await;
        if self.reject.load(Ordering::Acquire) {
            return Err(TinyBusError::failed("host rejected report"));
        }
        self.reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(report);
        Ok(SinkAck {})
    }
}

async fn connected_link(
    host: CompleteRecordingHost,
) -> Result<(Arc<TraceLink>, CompleteRecordingHost, Connection), Box<dyn std::error::Error>> {
    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    host_connection
        .serve_at(trace_sink::OBJECT_PATH.try_into()?, host.clone())
        .await?;
    host_connection.request_name(trace_sink::INTERFACE).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new_for_bus_tests(module, tinybus::BusName::new(trace_sink::INTERFACE)?)?;
    Ok((link, host, host_connection))
}

fn empty_task_report() -> Result<TaskReport, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "view": {
            "id": "task-1",
            "status": {"state": "running"},
            "summary": "working",
            "progress": 0.0,
            "next": []
        },
        "steps": [],
        "records": {},
        "artifacts": [],
        "learned": [],
        "trace": []
    }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callbacks_commit_events_artifacts_and_reports_over_the_bus()
-> Result<(), Box<dyn std::error::Error>> {
    let (link, host, _host_connection) = connected_link(CompleteRecordingHost::default()).await?;
    let deadline = TraceSinkDeadline::after(5_000)?;
    link.append(&serde_json::json!({"event": "started"}), deadline, 4096)
        .map_err(|error| error.to_string())?;
    let artifact_deadline = TraceSinkDeadline::after(5_000)?;
    let reference = link
        .store_artifact(
            TraceSinkArtifact {
                kind: EngineArtifactKind::Screenshot,
                name: "screen.png",
                bytes: &[1, 2, 3],
                deadline: artifact_deadline,
            },
            TraceSinkArtifactLimits {
                screenshot_bytes: 128,
                screenshot_count: 2,
                refmap_bytes: 64,
            },
        )
        .map_err(|error| error.to_string())?;
    link.write_report(TaskReportRecord {
        task_id: "task-1".into(),
        report: empty_task_report()?,
    })
    .await?;

    assert_eq!(reference, "trace-artifact-1");
    assert_eq!(
        host.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_slice(),
        &[serde_json::json!({"event": "started"})]
    );
    assert_eq!(
        host.artifacts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[0]
            .bytes,
        [1, 2, 3]
    );
    assert_eq!(
        host.reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[0]
            .task_id,
        "task-1"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_host_callbacks_return_errors_to_the_caller()
-> Result<(), Box<dyn std::error::Error>> {
    let host = CompleteRecordingHost::default();
    host.reject.store(true, Ordering::Release);
    let (link, _, _host_connection) = connected_link(host).await?;

    assert!(
        link.append(
            &serde_json::json!({"event": "rejected"}),
            TraceSinkDeadline::after(5_000)?,
            4096,
        )
        .is_err()
    );
    assert!(
        link.store_artifact(
            TraceSinkArtifact {
                kind: EngineArtifactKind::RefMap,
                name: "refs.json",
                bytes: &[4, 5],
                deadline: TraceSinkDeadline::after(5_000)?,
            },
            TraceSinkArtifactLimits {
                screenshot_bytes: 128,
                screenshot_count: 2,
                refmap_bytes: 64,
            },
        )
        .is_err()
    );
    assert!(
        link.write_report(TaskReportRecord {
            task_id: "task-1".into(),
            report: empty_task_report()?,
        })
        .await
        .is_err()
    );
    Ok(())
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.TraceSinkHost")]
impl RecordingHost {
    async fn append(&self, event: TraceEvent) -> TinyBusResult<SinkAck> {
        tokio::task::yield_now().await;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.record);
        Ok(SinkAck {})
    }

    async fn store_artifact(&self, _: TraceArtifact) -> TinyBusResult<TraceArtifactRef> {
        tokio::task::yield_now().await;
        Err(TinyBusError::failed("unused by deadline test"))
    }

    async fn write_report(&self, _: TaskReportRecord) -> TinyBusResult<SinkAck> {
        tokio::task::yield_now().await;
        Err(TinyBusError::failed("unused by deadline test"))
    }
}

#[test]
fn an_expired_deadline_is_never_renewed() {
    let deadline = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap_or_else(Instant::now);

    assert!(remaining_timeout(deadline).is_none());
    assert!(remaining_millis(deadline).is_none());
}

#[test]
fn artifacts_over_the_transport_safe_limit_are_refused_before_queueing() {
    let limit = tinycomputer_bus::trace_sink::MAX_TRACE_ARTIFACT_BYTES;

    assert!(validate_artifact_size(limit).is_ok());
    assert!(validate_artifact_size(limit + 1).is_err());
}

#[test]
fn callback_timeout_is_capped_without_extending_the_operation_deadline() {
    let deadline = Instant::now() + CALL_TIMEOUT * 2;
    assert!(remaining_timeout(deadline).is_some());
    let timeout = remaining_timeout(deadline).unwrap_or_default();

    assert!(timeout <= CALL_TIMEOUT);
    assert!(deadline > Instant::now());
}

#[test]
fn overlapping_callbacks_are_refused_until_the_outer_delivery_finishes() {
    let active = AtomicBool::new(false);

    {
        let outer = CallbackGuard::try_acquire(&active);
        assert!(outer.is_some());
        let Some(_outer) = outer else {
            return;
        };
        assert!(CallbackGuard::try_acquire(&active).is_none());
    }

    assert!(CallbackGuard::try_acquire(&active).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_already_expired_queued_event_is_rejected_without_a_host_write()
-> Result<(), Box<dyn std::error::Error>> {
    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    let host = RecordingHost::default();
    host_connection
        .serve_at(trace_sink::OBJECT_PATH.try_into()?, host.clone())
        .await?;
    host_connection.request_name(trace_sink::INTERFACE).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new_for_bus_tests(module, tinybus::BusName::new(trace_sink::INTERFACE)?)?;
    let (reply, result) = mpsc::sync_channel(1);
    link.worker.try_send(Command::Append {
        record: serde_json::json!({"event": "expired"}),
        deadline: Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap_or_else(Instant::now),
        byte_budget: 4096,
        reply,
    })?;
    let acknowledgment =
        tokio::task::spawn_blocking(move || result.recv_timeout(Duration::from_secs(1))).await??;

    assert!(acknowledgment.is_err());
    assert!(host.0.lock().map_err(|error| error.to_string())?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_queued_event_is_rejected_without_a_late_host_write()
-> Result<(), Box<dyn std::error::Error>> {
    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let host = BlockingRecordingHost {
        events: Arc::default(),
        started: Arc::new(std::sync::Mutex::new(Some(started_tx))),
        release: Arc::new(std::sync::Mutex::new(Some(release_rx))),
    };
    host_connection
        .serve_at(trace_sink::OBJECT_PATH.try_into()?, host.clone())
        .await?;
    host_connection.request_name(trace_sink::INTERFACE).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new_for_bus_tests(module, tinybus::BusName::new(trace_sink::INTERFACE)?)?;
    let (first_reply, first_result) = mpsc::sync_channel(1);
    link.worker.try_send(Command::Append {
        record: serde_json::json!({"event": "blocker"}),
        deadline: Instant::now() + Duration::from_secs(10),
        byte_budget: 4096,
        reply: first_reply,
    })?;
    tokio::time::timeout(Duration::from_secs(5), started_rx).await??;

    let (queued_reply, queued_result) = mpsc::sync_channel(1);
    let queued_deadline = Instant::now() + Duration::from_millis(50);
    link.worker.try_send(Command::Append {
        record: serde_json::json!({"event": "expired"}),
        deadline: queued_deadline,
        byte_budget: 4096,
        reply: queued_reply,
    })?;
    tokio::time::sleep_until(
        tokio::time::Instant::from_std(queued_deadline) + Duration::from_millis(1),
    )
    .await;
    release_tx
        .send(())
        .map_err(|()| TinyBusError::failed("test release receiver was dropped"))?;

    let first_ack =
        tokio::task::spawn_blocking(move || first_result.recv_timeout(Duration::from_secs(5)))
            .await??;
    let queued_ack =
        tokio::task::spawn_blocking(move || queued_result.recv_timeout(Duration::from_secs(5)))
            .await??;

    assert!(first_ack.is_ok());
    assert!(queued_ack.is_err());
    assert_eq!(
        host.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_slice(),
        &[serde_json::json!({"event": "blocker"})],
        "the expired queued event must not be written late"
    );
    Ok(())
}

#[derive(Clone)]
struct BlockingRecordingHost {
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    started: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    release: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.TraceSinkHost")]
impl BlockingRecordingHost {
    async fn append(&self, event: TraceEvent) -> TinyBusResult<SinkAck> {
        if event.record["event"] == "blocker" {
            if let Some(started) = self
                .started
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = started.send(());
            }
            let release = self
                .release
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(release) = release {
                release
                    .await
                    .map_err(|_| TinyBusError::failed("test callback was not released"))?;
            }
        }
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.record);
        Ok(SinkAck {})
    }

    async fn store_artifact(&self, _: TraceArtifact) -> TinyBusResult<TraceArtifactRef> {
        tokio::task::yield_now().await;
        Err(TinyBusError::failed("unused by queued deadline test"))
    }

    async fn write_report(&self, _: TaskReportRecord) -> TinyBusResult<SinkAck> {
        tokio::task::yield_now().await;
        Err(TinyBusError::failed("unused by queued deadline test"))
    }
}
