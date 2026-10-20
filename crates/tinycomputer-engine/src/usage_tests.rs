//! Per-task usage recorder tests.

use super::{ModelRole, UsageRecorder};

#[test]
fn snapshots_only_reported_measurements_and_counts_unknown_calls() {
    let recorder = UsageRecorder::default();
    recorder.record_started(ModelRole::Jev);
    recorder.record_measurements(ModelRole::Jev, Some(8), None, Some(3), None);
    recorder.record_call(ModelRole::Planner, None, None, None, None);

    let usage = recorder.snapshot();
    assert_eq!(usage.jev.calls, 1);
    assert_eq!(usage.jev.input_tokens, Some(8));
    assert_eq!(usage.jev.cached_input_tokens, None);
    assert_eq!(usage.jev.output_tokens, Some(3));
    assert_eq!(usage.jev.cost_usd_nanos, None);
    assert_eq!(usage.planner.calls, 1);
    assert_eq!(usage.planner.input_tokens, None);
}
