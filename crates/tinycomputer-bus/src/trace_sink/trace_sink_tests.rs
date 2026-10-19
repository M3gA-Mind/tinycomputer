use super::*;

#[test]
fn sink_records_keep_their_wire_shapes() {
    let event = TraceEvent {
        record: serde_json::json!({
            "event": "command.end", "seq": 4, "session_id": "s1"
        }),
        deadline_ms: 900,
        byte_budget: 4096,
    };
    let wire = serde_json::to_value(event).unwrap();
    assert_eq!(wire["record"]["event"], "command.end");
    assert_eq!(wire["deadline_ms"], 900);
    assert_eq!(wire["byte_budget"], 4096);

    let artifact = TraceArtifact {
        kind: TraceArtifactKind::RefMap,
        name: "snapshot.json".into(),
        bytes: vec![1, 2],
        deadline_ms: 700,
        limits: TraceArtifactLimits {
            screenshot_bytes: 10,
            screenshot_count: 2,
            refmap_bytes: 20,
        },
    };
    let wire = serde_json::to_value(artifact).unwrap();
    assert_eq!(wire["kind"], "ref_map");
    assert_eq!(wire["bytes"], serde_json::json!([1, 2]));
    assert_eq!(wire["limits"]["refmap_bytes"], 20);
}

#[test]
fn report_record_reuses_the_agent_report_contract() {
    let report = TaskReportRecord {
        task_id: "task-1".into(),
        report: serde_json::from_value(serde_json::json!({
            "view": {"id": "task-1"},
            "steps": [], "records": {}, "artifacts": [], "learned": [], "trace": []
        }))
        .unwrap(),
    };
    let wire = serde_json::to_value(report).unwrap();
    assert_eq!(wire["task_id"], "task-1");
    assert_eq!(wire["report"]["trace"], serde_json::json!([]));
}

#[test]
fn the_host_interface_is_separate_and_has_stable_member_names() {
    assert_ne!(INTERFACE, crate::names::INTERFACE);
    assert_eq!(METHODS, ["Append", "StoreArtifact", "WriteReport"]);
    assert_eq!(SinkAck {}, serde_json::from_str("{}").unwrap());
}
