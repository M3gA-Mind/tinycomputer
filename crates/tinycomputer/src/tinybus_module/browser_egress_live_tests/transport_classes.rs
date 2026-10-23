use super::*;

async fn serve_redirect_fixture(
    listener: tokio::net::TcpListener,
    port: u16,
    served: tokio::sync::oneshot::Sender<()>,
    complete: tokio::sync::oneshot::Sender<Vec<String>>,
) {
    let mut paths = Vec::new();
    let mut served = Some(served);
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let Ok(Ok(read)) = tokio::time::timeout(
                Duration::from_secs(5),
                tokio::io::AsyncReadExt::read(&mut stream, &mut chunk),
            )
            .await
            else {
                return;
            };
            if read == 0 {
                return;
            }
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&request);
        let Some(target) = request
            .lines()
            .next()
            .and_then(|line| line.split_ascii_whitespace().nth(1))
        else {
            return;
        };
        let path = url::Url::parse(target)
            .map(|url| url.path().to_owned())
            .unwrap_or_else(|_| target.to_owned());
        paths.push(path.clone());
        let response = match path.as_str() {
            "/redirect" => format!(
                "HTTP/1.1 302 Found\r\nLocation: http://fixture.test:{port}/page\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            ),
            "/page" => {
                let html = format!(
                    "<!doctype html><link rel=\"icon\" href=\"data:,\"><img src=\"http://fixture.test:{port}/pixel\"><script>new WebSocket(\"ws://fixture.test:{port}/socket\")</script>"
                );
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
                )
            }
            "/pixel" => {
                "HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".into()
            }
            // The test only needs to prove that Chromium's websocket handshake
            // reaches the upstream through the authorizing proxy. The fixture
            // refuses the handshake after observing the request.
            "/socket" => {
                "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".into()
            }
            _ => "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".into(),
        };
        if tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
            .await
            .is_err()
        {
            return;
        }
        if path == "/page" {
            if let Some(served) = served.take() {
                let _ = served.send(());
            }
        }
        if ["/redirect", "/page", "/pixel", "/socket"]
            .iter()
            .all(|expected| paths.iter().any(|path| path == expected))
        {
            let _ = complete.send(paths);
            return;
        }
    }
}

#[tokio::test]
async fn live_chromium_redirect_subresource_and_websocket_requests_use_host_authorization()
-> tinybus::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let allowed_address = listener.local_addr()?.to_string();
    let (complete_tx, complete_rx) = tokio::sync::oneshot::channel();
    let (served_tx, served_rx) = tokio::sync::oneshot::channel();
    let fixture = tokio::spawn(serve_redirect_fixture(
        listener,
        port,
        served_tx,
        complete_tx,
    ));
    let browser =
        start_browser_with_fixture_host(allowed_address, port, "/redirect", served_rx).await?;
    let paths = tokio::time::timeout(Duration::from_secs(10), complete_rx)
        .await
        .expect("redirect, page, subresource and websocket reach the fixture")
        .expect("fixture reports observed browser requests");
    let observations = browser.observations.lock().unwrap();
    let authorize_count = observations
        .iter()
        .filter(|call| call.starts_with("authorize:fixture-policy:41:fixture.test:"))
        .count();
    let consume_count = observations
        .iter()
        .filter(|call| call.starts_with("consume:"))
        .count();
    assert!(
        authorize_count >= 4,
        "each redirect, document, subresource and websocket must be authorized: {observations:?}"
    );
    assert!(
        consume_count >= 4,
        "each browser transport request must consume a socket grant: {observations:?}"
    );
    for expected in ["/redirect", "/page", "/pixel", "/socket"] {
        assert!(
            paths.iter().any(|path| path == expected),
            "missing {expected}: {paths:?}"
        );
    }
    drop(observations);

    let closed: DesktopResponse = browser
        .proxy
        .call(
            browser_names::methods::CLOSE_SESSION,
            (serde_json::json!({"session": browser.session}),),
        )
        .await?;
    assert!(
        closed.ok,
        "browser session did not close: {:?}",
        closed.error
    );
    browser.broker_task.abort();
    fixture.abort();
    Ok(())
}
