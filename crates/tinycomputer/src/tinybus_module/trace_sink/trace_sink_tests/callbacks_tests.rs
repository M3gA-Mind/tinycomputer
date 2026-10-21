//! Tests for trace-sink host callback delivery and rejection.

use super::*;

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

