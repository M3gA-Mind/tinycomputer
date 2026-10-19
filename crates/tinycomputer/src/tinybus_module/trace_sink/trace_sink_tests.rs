use super::*;
use std::sync::Mutex;
use tinycomputer_bus::trace_sink::TraceEvent;

#[derive(Clone, Default)]
struct RecordingHost(Arc<Mutex<Vec<serde_json::Value>>>);

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.TraceSinkHost")]
impl RecordingHost {
    async fn append(&self, event: TraceEvent) -> TinyBusResult<SinkAck> {
        self.0.lock().unwrap().push(event.record);
        Ok(SinkAck {})
    }

    async fn store_artifact(&self, _: TraceArtifact) -> TinyBusResult<TraceArtifactRef> {
        Err(TinyBusError::failed("unused by deadline test"))
    }

    async fn write_report(&self, _: TaskReportRecord) -> TinyBusResult<SinkAck> {
        Err(TinyBusError::failed("unused by deadline test"))
    }
}

#[test]
fn an_expired_deadline_is_never_renewed() {
    let deadline = Instant::now() - Duration::from_millis(1);

    assert!(remaining_timeout(deadline).is_none());
    assert!(remaining_millis(deadline).is_none());
}

#[test]
fn callback_timeout_is_capped_without_extending_the_operation_deadline() {
    let deadline = Instant::now() + CALL_TIMEOUT * 2;
    let timeout = remaining_timeout(deadline).expect("future deadline");

    assert!(timeout <= CALL_TIMEOUT);
    assert!(deadline > Instant::now());
}

#[test]
fn overlapping_callbacks_are_refused_until_the_outer_delivery_finishes() {
    let active = AtomicBool::new(false);

    {
        let _outer = CallbackGuard::try_acquire(&active).expect("first callback acquires guard");
        assert!(CallbackGuard::try_acquire(&active).is_none());
    }

    assert!(CallbackGuard::try_acquire(&active).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_queued_event_is_rejected_without_a_late_host_write() -> TinyBusResult<()> {
    let bus = tinybus::transport::memory::MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    let host = RecordingHost::default();
    host_connection
        .serve_at(trace_sink::OBJECT_PATH.try_into()?, host.clone())
        .await?;
    host_connection.request_name(trace_sink::INTERFACE).await?;
    let module = Connection::connect(bus.connect().await?).await?;
    let link = TraceLink::new(module, tinybus::BusName::new(trace_sink::INTERFACE)?)?;
    let (reply, result) = mpsc::sync_channel(1);
    link.worker
        .try_send(Command::Append {
            record: serde_json::json!({"event": "expired"}),
            deadline: Instant::now() - Duration::from_millis(1),
            byte_budget: 4096,
            reply,
        })
        .expect("worker accepts the expired command for rejection");
    let acknowledgment =
        tokio::task::spawn_blocking(move || result.recv_timeout(Duration::from_secs(1)))
            .await
            .expect("acknowledgment waiter completes")
            .expect("worker sends a result");

    assert!(acknowledgment.is_err());
    assert!(host.0.lock().unwrap().is_empty());
    Ok(())
}
