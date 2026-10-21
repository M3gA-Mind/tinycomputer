//! Forwards authorized proxy traffic and keeps active streams tied to their lease.

use std::{io, sync::Arc, time::Duration};

use base64::Engine as _;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::TcpStream,
};

use super::{EgressAuthorizer, request::ProxyRequest};

pub(super) async fn initialize_upstream(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    request: &ProxyRequest,
) -> io::Result<()> {
    if request.connect {
        write_all_bounded(
            client,
            b"HTTP/1.1 200 Connection Established\r\n\r\n",
            "browser CONNECT response timed out",
        )
        .await?;
        if request.bytes.len() > request.split {
            return write_all_bounded(
                upstream,
                &request.bytes[request.split..],
                "browser CONNECT payload timed out",
            )
            .await;
        }
        return Ok(());
    }
    let url = url::Url::parse(&request.uri)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP proxy URL"))?;
    let path = if let Some(query) = url.query() {
        format!("{}?{query}", url.path())
    } else {
        url.path().to_owned()
    };
    let mut forwarded = format!("{} {path} {}\r\n", request.method, request.version);
    for line in request
        .lines
        .iter()
        .skip(1)
        .map(String::as_str)
        .filter(|line| !line.is_empty())
    {
        let name = line.split_once(':').map_or("", |(name, _)| name);
        if !name.eq_ignore_ascii_case("connection")
            && !name.eq_ignore_ascii_case("proxy-connection")
            && !name.eq_ignore_ascii_case("proxy-authorization")
        {
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
    }
    if request.has_upgrade {
        forwarded.push_str("Connection: Upgrade\r\n\r\n");
    } else {
        forwarded.push_str("Connection: close\r\n\r\n");
    }
    write_all_bounded(
        upstream,
        forwarded.as_bytes(),
        "browser upstream request timed out",
    )
    .await?;
    write_all_bounded(
        upstream,
        &request.bytes[request.split..],
        "browser upstream body timed out",
    )
    .await
}
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

pub(super) async fn tunnel_with_lease(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    auth: &Arc<dyn EgressAuthorizer>,
    lease: String,
) -> io::Result<()> {
    let relay = copy_bidirectional(client, upstream);
    tokio::pin!(relay);
    let mut validations = validation_interval();
    loop {
        tokio::select! {
            biased;
            _ = validations.tick() => {
                if !validate_lease(auth, &lease).await? { return Ok(()); }
            },
            result = &mut relay => { result?; return Ok(()); },
        }
    }
}

pub(super) async fn forward_response_with_lease(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    auth: &Arc<dyn EgressAuthorizer>,
    lease: String,
) -> io::Result<()> {
    let mut chunk = [0u8; 8192];
    let mut validations = validation_interval();
    loop {
        tokio::select! {
            biased;
            _ = validations.tick() => {
                if !validate_lease(auth, &lease).await? { return Ok(()); }
            },
            result = upstream.read(&mut chunk) => {
                let n = result?;
                if n == 0 { return Ok(()); }
                write_all_bounded(client, &chunk[..n], "browser response write timed out").await?;
            },
        }
    }
}

fn validation_interval() -> tokio::time::Interval {
    let period = Duration::from_secs(1);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

pub(super) async fn validate_lease(
    auth: &Arc<dyn EgressAuthorizer>,
    lease: &str,
) -> io::Result<bool> {
    tokio::time::timeout(Duration::from_secs(2), auth.validate(lease.to_owned()))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "browser lease validation timed out",
            )
        })?
}
pub(super) async fn refuse(client: &mut TcpStream) -> io::Result<()> {
    write_all_bounded(
        client,
        b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        "browser refusal response timed out",
    )
    .await
}

pub(super) async fn write_all_bounded(
    stream: &mut TcpStream,
    bytes: &[u8],
    message: &'static str,
) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), stream.write_all(bytes))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, message))?
}
