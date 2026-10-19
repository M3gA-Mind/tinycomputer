use super::*;
use crate::{
    TraceSink, TraceSinkArtifact, TraceSinkArtifactLimits, TraceSinkDeadline, TraceSinkError,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct RecordingSink {
    events: Mutex<Vec<Value>>,
}

impl TraceSink for RecordingSink {
    fn append(
        &self,
        event: &Value,
        _deadline: TraceSinkDeadline,
        _byte_budget: u64,
    ) -> std::result::Result<(), TraceSinkError> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }

    fn store_artifact(
        &self,
        _artifact: TraceSinkArtifact<'_>,
        _limits: TraceSinkArtifactLimits,
    ) -> std::result::Result<String, TraceSinkError> {
        Ok("opaque:test".into())
    }
}

#[test]
fn opted_in_sink_receives_events_without_opening_the_configured_trace_path() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "tinycomputer-native-trace-{}-{nonce}.jsonl",
        std::process::id(),
    ));
    let _ = std::fs::remove_file(&path);
    let sink = Arc::new(RecordingSink::default());
    let desktop = Desktop::new()
        .with_trace(path.clone(), true)
        .with_trace_sink(sink.clone());

    let reply = desktop.version();

    assert!(reply.ok, "a committed trace event lets the command succeed");
    assert!(desktop.is_tracing());
    assert!(!path.exists(), "sink mode must not create the legacy file");
    assert!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "command.end")
    );
}
