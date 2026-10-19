//! Tests for the launch a session starts with.

use serde_json::json;
use tinycomputer_bus::browser::{
    Screen, SessionOptions, UserAgentBrand, UserAgentMetadata, Viewport,
};

use crate::convert::{launch, viewport};

#[test]
fn a_default_session_launches_headless_with_nothing_else() {
    assert_eq!(
        launch(&SessionOptions::default()),
        json!({"action": "launch", "headless": true, "args": []})
    );
}

#[test]
fn every_session_option_reaches_its_launch_field() {
    let options = SessionOptions {
        endpoint: Some("ws://127.0.0.1:9222".to_owned()),
        executable: Some("/usr/bin/chromium".to_owned()),
        headless: false,
        user_agent: Some("agent".to_owned()),
        user_data_dir: Some("/profiles/trip".to_owned()),
        download_dir: Some("/downloads".to_owned()),
        args: vec!["--lang=en".to_owned()],
        allowed_origins: vec!["https://example.com".to_owned()],
        viewport: Viewport::desktop(800, 600),
        ..SessionOptions::default()
    };
    assert_eq!(
        launch(&options),
        json!({
            "action": "launch",
            "headless": false,
            "args": ["--lang=en"],
            "cdpUrl": "ws://127.0.0.1:9222",
            "executablePath": "/usr/bin/chromium",
            "userAgent": "agent",
            "profile": "/profiles/trip",
            "downloadPath": "/downloads",
        }),
        "the allowed origins stay with the session, which checks pages itself"
    );
    assert_eq!(
        viewport(&options),
        json!({"action": "viewport", "width": 800, "height": 600, "deviceScaleFactor": 1.0, "mobile": false})
    );
}

#[test]
fn every_identity_option_reaches_its_launch_field() {
    let options = SessionOptions {
        timezone: Some("Asia/Tokyo".to_owned()),
        locale: Some("ja-JP".to_owned()),
        accept_language: Some("ja-JP,ja".to_owned()),
        user_agent_metadata: Some(UserAgentMetadata {
            brands: Some(vec![UserAgentBrand {
                brand: "Chromium".to_owned(),
                version: "140".to_owned(),
            }]),
            full_version_list: Some(vec![UserAgentBrand {
                brand: "Chromium".to_owned(),
                version: "140.0.7339.0".to_owned(),
            }]),
            full_version: None,
            platform: "macOS".to_owned(),
            platform_version: "14.5.0".to_owned(),
            architecture: "arm".to_owned(),
            model: String::new(),
            mobile: false,
            bitness: Some("64".to_owned()),
            wow64: Some(false),
        }),
        screen: Some(Screen {
            width: 1920,
            height: 1080,
            device_scale_factor: 2.0,
        }),
        init_scripts: vec!["window.marker = 1;".to_owned()],
        stealth: true,
        ..SessionOptions::default()
    };
    assert_eq!(
        launch(&options),
        json!({
            "action": "launch",
            "headless": true,
            "args": [],
            "timezone": "Asia/Tokyo",
            "locale": "ja-JP",
            "acceptLanguage": "ja-JP,ja",
            "userAgentMetadata": {
                "brands": [{"brand": "Chromium", "version": "140"}],
                "fullVersionList": [{"brand": "Chromium", "version": "140.0.7339.0"}],
                "platform": "macOS",
                "platformVersion": "14.5.0",
                "architecture": "arm",
                "model": "",
                "mobile": false,
                "bitness": "64",
                "wow64": false,
            },
            "screen": {"width": 1920, "height": 1080, "deviceScaleFactor": 2.0},
            "initScriptSources": ["window.marker = 1;"],
            "stealth": true,
        })
    );
}

#[test]
fn a_session_that_asks_for_no_identity_sends_none() {
    // `stealth: false` and no init scripts are the defaults: none is sent, so
    // an engine that predates the fields never sees them.
    let command = launch(&SessionOptions::default());
    let sent = command.as_object().expect("an object");
    for key in [
        "timezone",
        "locale",
        "acceptLanguage",
        "userAgentMetadata",
        "screen",
        "initScriptSources",
        "stealth",
    ] {
        assert!(!sent.contains_key(key), "{key} was sent");
    }
}

#[test]
fn a_screen_sets_the_pixel_ratio_the_viewport_would_otherwise_replace() {
    // The viewport is applied after the launch and replaces its pixel ratio,
    // so a session that asked for a 2x screen would report 1x.
    let options = SessionOptions {
        screen: Some(Screen {
            width: 1920,
            height: 1080,
            device_scale_factor: 2.0,
        }),
        ..SessionOptions::default()
    };
    assert_eq!(
        viewport(&options),
        json!({"action": "viewport", "width": 1280, "height": 800, "deviceScaleFactor": 2.0, "mobile": false})
    );
    assert_eq!(
        viewport(&SessionOptions::default())["deviceScaleFactor"],
        1.0,
        "without a screen the viewport's own ratio stands"
    );
}
