//! Tests for the identity a session asks its browser to present: the launch
//! command carries it, and a real browser shows it to the page (gated on the
//! `agent-browser` feature and `TINYCOMPUTER_LIVE_BROWSER=1`, since CI has no
//! browser to launch).

use tinycomputer_bus::browser::{Screen, SessionOptions, UserAgentBrand, UserAgentMetadata};

use crate::fake::Fake;
use crate::sessions::Browser;

use super::scratch;

fn tokyo() -> SessionOptions {
    SessionOptions {
        timezone: Some("Asia/Tokyo".to_owned()),
        locale: Some("ja-JP".to_owned()),
        accept_language: Some("ja-JP,ja".to_owned()),
        user_agent: Some(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like \
             Gecko) Chrome/140.0.0.0 Safari/537.36"
                .to_owned(),
        ),
        user_agent_metadata: Some(UserAgentMetadata {
            brands: Some(vec![UserAgentBrand {
                brand: "Chromium".to_owned(),
                version: "140".to_owned(),
            }]),
            full_version_list: None,
            full_version: Some("140.0.0.0".to_owned()),
            platform: "macOS".to_owned(),
            platform_version: "14.5.0".to_owned(),
            architecture: "arm".to_owned(),
            model: String::new(),
            mobile: false,
            bitness: Some("64".to_owned()),
            wow64: None,
        }),
        screen: Some(Screen {
            width: 1920,
            height: 1080,
            device_scale_factor: 2.0,
        }),
        init_scripts: vec!["window.fromInitScript = 'ran first';".to_owned()],
        stealth: true,
        ..SessionOptions::default()
    }
}

#[tokio::test]
async fn opening_a_session_hands_the_engine_every_identity_field_in_its_launch() {
    let fake = Fake::new();
    let browser = Browser::with_scratch(std::sync::Arc::new(fake.clone()), scratch("identity"));
    browser.open_session(tokyo()).await.unwrap();

    let launch = fake.last("launch");
    assert_eq!(launch["timezone"], "Asia/Tokyo");
    assert_eq!(launch["locale"], "ja-JP");
    assert_eq!(launch["acceptLanguage"], "ja-JP,ja");
    assert_eq!(launch["userAgentMetadata"]["platform"], "macOS");
    assert_eq!(launch["screen"]["width"], 1920);
    assert_eq!(
        launch["initScriptSources"][0],
        "window.fromInitScript = 'ran first';"
    );
    assert_eq!(launch["stealth"], true);
}

/// A page on `localhost`, served to the first few requests.
#[cfg(feature = "agent-browser")]
fn serve_a_page() -> String {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let body = "<!doctype html><title>identity</title><p>identity</p>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://localhost:{port}/")
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_page_sees_the_identity_its_session_was_opened_with() {
    use std::sync::Arc;

    use serde_json::json;

    if std::env::var("TINYCOMPUTER_LIVE_BROWSER").as_deref() != Ok("1") {
        return;
    }
    let browser = Browser::new(Arc::new(crate::AgentBrowser));
    let options = SessionOptions {
        executable: std::env::var("TINYCOMPUTER_BROWSER_EXECUTABLE").ok(),
        ..tokyo()
    };
    let info = browser
        .open_session(options)
        .await
        .expect("a browser launches when live runs are asked for");
    // Init scripts run in a document loaded after launch, not the blank tab,
    // and client hints exist only in a secure context, which a page on
    // localhost is and a `data:` page is not.
    let url = serve_a_page();
    browser
        .command(&info.id, json!({"action": "navigate", "url": url}))
        .await
        .expect("the page loads");
    let read = |expression: &'static str| {
        let browser = &browser;
        let id = &info.id;
        async move {
            browser
                .command(id, json!({"action": "evaluate", "script": expression}))
                .await
                .expect("the expression runs")["result"]
                .clone()
        }
    };

    assert_eq!(
        read("Intl.DateTimeFormat().resolvedOptions().timeZone").await,
        "Asia/Tokyo"
    );
    assert_eq!(read("navigator.language").await, "ja-JP");
    assert_eq!(read("navigator.languages.join(',')").await, "ja-JP,ja");
    assert_eq!(read("navigator.userAgentData.platform").await, "macOS");
    assert_eq!(
        read("navigator.userAgentData.brands.some(b => b.brand === 'Chromium' && b.version === '140')")
            .await,
        true
    );
    assert_eq!(read("screen.width").await, 1920);
    assert_eq!(read("screen.height").await, 1080);
    assert_eq!(read("window.devicePixelRatio").await, 2);
    assert_eq!(read("window.fromInitScript").await, "ran first");
    browser.close_session(&info.id).await.unwrap();
}
