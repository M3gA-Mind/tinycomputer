//! Bounded HTTP/1.1 chunk framing validation and forwarding.

use std::{io, time::Duration};

use tokio::io::AsyncReadExt;

use crate::egress::forward::write_all_bounded;

pub(super) async fn forward_chunked_request_body<C, U>(
    client: &mut C,
    upstream: &mut U,
    initial: &[u8],
) -> io::Result<()>
where
    C: tokio::io::AsyncRead + Unpin,
    U: tokio::io::AsyncWrite + Unpin,
{
    let mut pending = initial.to_vec();
    let mut offset = 0;
    let mut trailer_bytes = 0;
    loop {
        let line = read_chunk_line(client, &mut pending, &mut offset).await?;
        let size_text = std::str::from_utf8(&line[..line.len() - 2])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        let size_text = size_text.split(';').next().unwrap_or_default();
        if size_text.is_empty() || !size_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk size",
            ));
        }
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        write_all_bounded(upstream, &line, "browser upstream body timed out").await?;
        if size == 0 {
            loop {
                let trailer = read_chunk_line(client, &mut pending, &mut offset).await?;
                trailer_bytes += trailer.len();
                let trailer_line = &trailer[..trailer.len() - 2];
                let forbidden_trailer = std::str::from_utf8(trailer_line)
                    .ok()
                    .and_then(|line| line.split_once(':'))
                    .is_some_and(|(name, _)| {
                        name.is_empty()
                            || name.eq_ignore_ascii_case("content-length")
                            || name.eq_ignore_ascii_case("transfer-encoding")
                            || name.eq_ignore_ascii_case("host")
                    });
                if trailer_bytes > 32 * 1024
                    || (trailer != b"\r\n" && (!trailer_line.contains(&b':') || forbidden_trailer))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid chunk trailer",
                    ));
                }
                write_all_bounded(upstream, &trailer, "browser upstream body timed out").await?;
                if trailer == b"\r\n" {
                    if offset != pending.len() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "bytes after chunked request body",
                        ));
                    }
                    return Ok(());
                }
            }
        }
        let mut remaining = size;
        let mut chunk = [0u8; 8192];
        while remaining > 0 {
            let buffered = pending.len().saturating_sub(offset);
            let n = if buffered > 0 {
                let n = buffered.min(remaining).min(chunk.len());
                chunk[..n].copy_from_slice(&pending[offset..offset + n]);
                offset += n;
                n
            } else {
                let read_length = remaining.min(chunk.len());
                let n = tokio::time::timeout(
                    Duration::from_secs(10),
                    client.read(&mut chunk[..read_length]),
                )
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy body timed out"))??;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "browser request body ended early",
                    ));
                }
                n
            };
            write_all_bounded(upstream, &chunk[..n], "browser upstream body timed out").await?;
            remaining -= n;
        }
        let ending = read_body_bytes(client, &mut pending, &mut offset, 2).await?;
        if ending != b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk ending",
            ));
        }
        write_all_bounded(upstream, &ending, "browser upstream body timed out").await?;
    }
}

async fn read_chunk_line<C: tokio::io::AsyncRead + Unpin>(
    client: &mut C,
    pending: &mut Vec<u8>,
    offset: &mut usize,
) -> io::Result<Vec<u8>> {
    loop {
        if let Some(end) = pending[*offset..]
            .windows(2)
            .position(|window| window == b"\r\n")
        {
            let end = *offset + end + 2;
            let line = pending[*offset..end].to_vec();
            *offset = end;
            return Ok(line);
        }
        if pending.len().saturating_sub(*offset) > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk line is too long",
            ));
        }
        if *offset > 8192 {
            pending.drain(..*offset);
            *offset = 0;
        }
        let mut chunk = [0u8; 1024];
        let n = tokio::time::timeout(Duration::from_secs(10), client.read(&mut chunk))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy body timed out"))??;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "browser request body ended early",
            ));
        }
        pending.extend_from_slice(&chunk[..n]);
    }
}

async fn read_body_bytes<C: tokio::io::AsyncRead + Unpin>(
    client: &mut C,
    pending: &mut Vec<u8>,
    offset: &mut usize,
    length: usize,
) -> io::Result<Vec<u8>> {
    while pending.len().saturating_sub(*offset) < length {
        if *offset > 8192 {
            pending.drain(..*offset);
            *offset = 0;
        }
        let mut chunk = [0u8; 1024];
        let n = tokio::time::timeout(Duration::from_secs(10), client.read(&mut chunk))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy body timed out"))??;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "browser request body ended early",
            ));
        }
        pending.extend_from_slice(&chunk[..n]);
    }
    let end = *offset + length;
    let bytes = pending[*offset..end].to_vec();
    *offset = end;
    Ok(bytes)
}
