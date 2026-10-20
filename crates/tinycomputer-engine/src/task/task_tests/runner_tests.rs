//! Tests for what a `FlowRunner` gets by default when it only runs flows.

use super::*;

/// A runner that implements only `run`, leaving the rest to the trait.
struct RunsOnly;

impl FlowRunner for RunsOnly {
    fn run(
        &self,
        _task: &TaskId,
        _constraints: &TaskConstraints,
        _request: RunFlowRequest,
    ) -> FlowFuture {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn a_runner_that_only_runs_flows_reads_nothing_and_holds_nothing() {
    let task = TaskId::new("t-1");
    assert_eq!(
        RunsOnly.visible_text(&task).await,
        [] as [std::string::String; 0]
    );
    assert!(RunsOnly.capture(&task).await.is_none());
    // Releasing a task the runner never held is a no-op, not a failure.
    RunsOnly.release(&task);
    // Nothing to get ready, and no journal to write to.
    RunsOnly.prepare(&task, &TaskConstraints::default()).await;
    RunsOnly.open_page(&task, "https://mail.test").await;
    RunsOnly.warm(&task, 7).await;
    assert_eq!(RunsOnly.redact(&task, "ordinary text"), "ordinary text");
    RunsOnly.journal(Some(&task), "plan", &|| serde_json::json!({"wall_ms": 1}));
    RunsOnly.journal(None, "plan", &|| serde_json::json!({}));
    assert_eq!(
        RunsOnly.visible_text(&task).await,
        [] as [std::string::String; 0]
    );
}
