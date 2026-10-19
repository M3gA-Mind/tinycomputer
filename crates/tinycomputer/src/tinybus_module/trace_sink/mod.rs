//! The optional host-owned persistence path for desktop traces and reports.
//!
//! The desktop engine emits trace events synchronously, sometimes while a
//! task is on a Tokio executor thread. This port moves every trace callback to
//! one dedicated worker before making a bounded synchronous wait, so the bus
//! acknowledgement never runs on the caller's executor thread. One worker
//! preserves event order for this module instance.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tinybus::{Connection, Error as TinyBusError, Proxy, Result as TinyBusResult};
use tinycomputer_bus::trace_sink::{
    self, SinkAck, TaskReportRecord, TraceArtifact, TraceArtifactKind, TraceArtifactRef, TraceEvent,
};
use tinycomputer_desktop::{
    TraceSink as EngineTraceSink, TraceSinkArtifact, TraceSinkArtifactKind as EngineArtifactKind,
    TraceSinkArtifactLimits, TraceSinkDeadline, TraceSinkError,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

enum Command {
    Append(TraceEvent, SyncSender<Result<(), ()>>),
    StoreArtifact(TraceArtifact, SyncSender<Result<String, ()>>),
}

/// The synchronous sink adapter, backed by a worker that owns the bus calls.
#[derive(Clone)]
pub(crate) struct TraceLink {
    connection: Connection,
    worker: SyncSender<Command>,
}

impl std::fmt::Debug for TraceLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("TraceLink").finish_non_exhaustive()
    }
}

impl TraceLink {
    pub(crate) fn new(connection: Connection) -> TinyBusResult<Arc<Self>> {
        let worker_connection = connection.clone();
        let runtime = tokio::runtime::Handle::current();
        if runtime.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
            return Err(TinyBusError::failed(
                "trace sink requires a multi-thread Tokio runtime",
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("tinycomputer-trace-sink".into())
            .spawn(move || worker(runtime, worker_connection, receiver))
            .map_err(|error| TinyBusError::failed(format!("trace sink worker failed: {error}")))?;
        Ok(Arc::new(Self {
            connection,
            worker: sender,
        }))
    }

    pub(crate) async fn write_report(&self, record: TaskReportRecord) -> TinyBusResult<()> {
        call::<SinkAck, _>(
            &self.proxy(CALL_TIMEOUT)?,
            trace_sink::methods::WRITE_REPORT,
            (record,),
        )
        .await
        .map(|_| ())
    }

    fn proxy(&self, timeout: Duration) -> TinyBusResult<Proxy> {
        self.connection
            .proxy(
                trace_sink::INTERFACE,
                trace_sink::OBJECT_PATH,
                trace_sink::INTERFACE,
            )
            .map(|proxy| proxy.with_timeout(timeout))
    }
}

fn worker(runtime: tokio::runtime::Handle, connection: Connection, receiver: Receiver<Command>) {
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Append(record, reply) => {
                let timeout = timeout(record.deadline_ms);
                let result = runtime.block_on(async {
                    let proxy = connection
                        .proxy(
                            trace_sink::INTERFACE,
                            trace_sink::OBJECT_PATH,
                            trace_sink::INTERFACE,
                        )?
                        .with_timeout(timeout);
                    call::<SinkAck, _>(&proxy, trace_sink::methods::APPEND, (record,))
                        .await
                        .map(|_| ())
                });
                let _ = reply.send(result.map_err(|_| ()));
            }
            Command::StoreArtifact(artifact, reply) => {
                let timeout = timeout(artifact.deadline_ms);
                let result = runtime.block_on(async {
                    let proxy = connection
                        .proxy(
                            trace_sink::INTERFACE,
                            trace_sink::OBJECT_PATH,
                            trace_sink::INTERFACE,
                        )?
                        .with_timeout(timeout);
                    call::<TraceArtifactRef, _>(
                        &proxy,
                        trace_sink::methods::STORE_ARTIFACT,
                        (artifact,),
                    )
                    .await
                    .map(|reply| reply.reference)
                });
                let _ = reply.send(result.map_err(|_| ()));
            }
        }
    }
}

#[cfg(test)]
async fn call<T, A>(proxy: &Proxy, member: &str, args: A) -> TinyBusResult<T>
where
    T: DeserializeOwned,
    A: Serialize,
{
    proxy.call(member, args).await
}

#[cfg(not(test))]
async fn call<T, A>(proxy: &Proxy, member: &str, args: A) -> TinyBusResult<T>
where
    T: DeserializeOwned,
    A: Serialize,
{
    proxy.call_confidential(member, args).await
}

fn timeout(deadline_ms: u64) -> Duration {
    Duration::from_millis(deadline_ms.max(1)).min(CALL_TIMEOUT)
}

fn remaining_ms(deadline: TraceSinkDeadline) -> u64 {
    u64::try_from(deadline.remaining().as_millis()).unwrap_or(u64::MAX)
}

fn receive<T>(receiver: &mpsc::Receiver<T>, timeout: Duration) -> Result<T, RecvTimeoutError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| receiver.recv_timeout(timeout))
        }
        // A current-thread executor cannot make progress while this sync
        // callback waits, so fail instead of deadlocking its dispatch loop.
        Ok(_) => Err(RecvTimeoutError::Timeout),
        Err(_) => receiver.recv_timeout(timeout),
    }
}

impl EngineTraceSink for TraceLink {
    fn append(
        &self,
        record: &serde_json::Value,
        deadline: TraceSinkDeadline,
        byte_budget: u64,
    ) -> std::result::Result<(), TraceSinkError> {
        let deadline_ms = remaining_ms(deadline);
        let (reply, result) = mpsc::sync_channel(1);
        self.worker
            .try_send(Command::Append(
                TraceEvent {
                    record: record.clone(),
                    deadline_ms,
                    byte_budget,
                },
                reply,
            ))
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    TraceSinkError::Internal("trace sink worker is busy".into())
                }
                TrySendError::Disconnected(_) => {
                    TraceSinkError::Internal("trace sink worker stopped".into())
                }
            })?;
        receive(&result, timeout(deadline_ms))
            .map_err(|_| TraceSinkError::Internal("trace sink acknowledgment timed out".into()))?
            .map_err(|_| TraceSinkError::Internal("trace sink rejected event".into()))
    }

    fn store_artifact(
        &self,
        artifact: TraceSinkArtifact<'_>,
        limits: TraceSinkArtifactLimits,
    ) -> std::result::Result<String, TraceSinkError> {
        let deadline_ms = remaining_ms(artifact.deadline);
        let (reply, result) = mpsc::sync_channel(1);
        let kind = match artifact.kind {
            EngineArtifactKind::Screenshot => TraceArtifactKind::Screenshot,
            EngineArtifactKind::RefMap => TraceArtifactKind::RefMap,
        };
        self.worker
            .try_send(Command::StoreArtifact(
                TraceArtifact {
                    kind,
                    name: artifact.name.to_owned(),
                    bytes: artifact.bytes.to_vec(),
                    deadline_ms,
                    limits: tinycomputer_bus::trace_sink::TraceArtifactLimits {
                        screenshot_bytes: limits.screenshot_bytes,
                        screenshot_count: limits.screenshot_count,
                        refmap_bytes: limits.refmap_bytes,
                    },
                },
                reply,
            ))
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    TraceSinkError::Internal("trace sink worker is busy".into())
                }
                TrySendError::Disconnected(_) => {
                    TraceSinkError::Internal("trace sink worker stopped".into())
                }
            })?;
        receive(&result, timeout(deadline_ms))
            .map_err(|_| {
                TraceSinkError::Internal("trace artifact acknowledgment timed out".into())
            })?
            .map_err(|_| TraceSinkError::Internal("trace sink rejected artifact".into()))
    }
}

#[cfg(test)]
#[path = "trace_sink_tests.rs"]
mod tests;
