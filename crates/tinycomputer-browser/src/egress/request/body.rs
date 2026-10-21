//! Streams validated fixed-length and chunked request bodies with bounded memory.

use std::{io, time::Duration};

use tokio::io::AsyncReadExt;

use crate::egress::forward::write_all_bounded;

mod chunked;

#[cfg(test)]
mod chunked_tests;

use super::ProxyRequest;

pub(in crate::egress) fn has_valid_buffered_body(request: &ProxyRequest) -> bool {
    if request.has_upgrade || request.connect || request.chunked {
        return true;
    }
    let received = request.bytes.len().saturating_sub(request.split);
    received <= request.body_len
}

pub(in crate::egress) fn buffered_body_len(request: &ProxyRequest) -> usize {
    if request.has_upgrade || request.connect || request.chunked {
        0
    } else {
        request.bytes.len().saturating_sub(request.split)
    }
}

pub(in crate::egress) async fn read_request_body<C, U>(
    client: &mut C,
    upstream: &mut U,
    request: &ProxyRequest,
    mut received: usize,
) -> io::Result<()>
where
    C: tokio::io::AsyncRead + Unpin,
    U: tokio::io::AsyncWrite + Unpin,
{
    if request.has_upgrade || request.connect {
        return Ok(());
    }
    if request.chunked {
        return chunked::forward_chunked_request_body(
            client,
            upstream,
            &request.bytes[request.split..],
        )
        .await;
    }
    let mut chunk = [0u8; 1024];
    while received < request.body_len {
        let n = tokio::time::timeout(Duration::from_secs(10), client.read(&mut chunk))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy body timed out"))??;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "browser request body ended early",
            ));
        }
        let remaining = request.body_len - received;
        if n > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "browser request body exceeded content length",
            ));
        }
        write_all_bounded(upstream, &chunk[..n], "browser upstream body timed out").await?;
        received += n;
    }
    Ok(())
}
