//! Lease validation tests for response forwarding.

use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::super::{BrowserDialTarget, DialGrant, EgressAuthorizer};

struct RevokingAuthorizer(AtomicUsize);

impl EgressAuthorizer for RevokingAuthorizer {
    fn authorize(
        &self,
        _target: BrowserDialTarget,
    ) -> Pin<Box<dyn Future<Output = io::Result<DialGrant>> + Send>> {
        Box::pin(async { Err(io::Error::other("unused authorization")) })
    }

    fn consume(
        &self,
        _target: BrowserDialTarget,
        _grant: DialGrant,
        _address: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send>> {
        Box::pin(async { Err(io::Error::other("unused grant")) })
    }

    fn validate(&self, _lease: String) -> Pin<Box<dyn Future<Output = io::Result<bool>> + Send>> {
        let validation = self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(validation == 0) })
    }
}

struct BlockedWriter;

impl AsyncWrite for BlockedWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test(start_paused = true)]
async fn validates_lease_while_response_write_is_blocked() {
    let (mut upstream_writer, upstream) = tokio::io::duplex(32);
    upstream_writer.write_all(b"response").await.unwrap();
    drop(upstream_writer);

    let authorizer = Arc::new(RevokingAuthorizer(AtomicUsize::new(0)));
    let validation_count = Arc::clone(&authorizer);
    let authorizer: Arc<dyn EgressAuthorizer> = authorizer;
    let task = tokio::spawn(async move {
        let mut upstream = upstream;
        super::forward_response_with_lease(
            &mut BlockedWriter,
            &mut upstream,
            &authorizer,
            "lease".to_owned(),
        )
        .await
    });

    tokio::task::yield_now().await;
    assert_eq!(validation_count.0.load(Ordering::SeqCst), 0);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(validation_count.0.load(Ordering::SeqCst), 1);
    assert!(!task.is_finished());

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert!(task.await.unwrap().is_ok());
    assert_eq!(validation_count.0.load(Ordering::SeqCst), 2);
}
