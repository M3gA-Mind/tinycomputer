#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

// ── Guard clause: empty / whitespace input short-circuits ────
//
// The post-guard code (clipboard / enigo / AppleScript) needs a
// display and a real system event loop, so coverage of those paths
// below `insert_text`'s trim-guard is only achievable in an
// end-to-end integration environment. Units here pin the logic
// that IS deterministic in a headless test process.

#[test]
fn empty_text_is_noop_and_succeeds() {
    assert!(insert_text("", None).is_ok());
}

#[test]
fn whitespace_only_skips_insertion_and_succeeds() {
    assert!(insert_text("   ", None).is_ok());
}

#[test]
fn newlines_and_tabs_only_also_treated_as_empty() {
    // `trim()` strips any Unicode whitespace — the skip branch must
    // fire for pure `\t` and `\n` buffers too, not just spaces.
    assert!(insert_text("\n\n", None).is_ok());
    assert!(insert_text("\t  \n", Some("any-app")).is_ok());
}

#[test]
fn paste_modifier_is_platform_correct() {
    let key = paste_modifier_key();
    if cfg!(target_os = "macos") {
        assert!(matches!(key, Key::Meta));
    } else {
        assert!(matches!(key, Key::Control));
    }
}

#[test]
fn constants_match_openwhispr_timings() {
    // Lock in the OpenWhispr-derived delays so nobody silently
    // shortens them (would race the target app's paste handler).
    assert_eq!(PASTE_DELAY, Duration::from_millis(120));
    assert_eq!(CLIPBOARD_RESTORE_DELAY, Duration::from_millis(450));
}

// ── The paste flow against a fake clipboard and keyboard ─────

use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake {
    clipboard: Mutex<Option<String>>,
    keys: Mutex<Vec<(String, String)>>,
    opens: Mutex<u32>,
    writes: Mutex<u32>,
    fail_open_at: Option<u32>,
    fail_write_at: Option<u32>,
    fail_key_at: Option<usize>,
    fail_keyboard: bool,
}

#[derive(Clone, Default)]
struct FakePlatform(Arc<Fake>);

struct FakeBoard(Arc<Fake>);
struct FakeKeys(Arc<Fake>);

impl Pasteboard for FakeBoard {
    fn read(&mut self) -> Option<String> {
        self.0.clipboard.lock().unwrap().clone()
    }
    fn write(&mut self, text: &str) -> Result<(), String> {
        let mut writes = self.0.writes.lock().unwrap();
        let index = *writes;
        *writes += 1;
        if self.0.fail_write_at == Some(index) {
            return Err("denied".into());
        }
        *self.0.clipboard.lock().unwrap() = Some(text.to_string());
        Ok(())
    }
}

impl KeyOps for FakeKeys {
    fn key(&mut self, key: Key, direction: Direction) -> Result<(), String> {
        let mut keys = self.0.keys.lock().unwrap();
        if self.0.fail_key_at == Some(keys.len()) {
            return Err("key refused".into());
        }
        keys.push((format!("{key:?}"), format!("{direction:?}")));
        Ok(())
    }
}

impl Platform for FakePlatform {
    fn pasteboard(&self) -> Result<Box<dyn Pasteboard>, String> {
        let mut opens = self.0.opens.lock().unwrap();
        let index = *opens;
        *opens += 1;
        if self.0.fail_open_at == Some(index) {
            return Err("no clipboard".into());
        }
        Ok(Box::new(FakeBoard(self.0.clone())))
    }
    fn keyboard(&self) -> Result<Box<dyn KeyOps>, String> {
        if self.0.fail_keyboard {
            return Err("no keyboard".into());
        }
        Ok(Box::new(FakeKeys(self.0.clone())))
    }
}

const NONE: Duration = Duration::ZERO;

fn platform(configure: impl FnOnce(&mut Fake)) -> FakePlatform {
    let mut fake = Fake::default();
    *fake.clipboard.get_mut().unwrap() = Some("previous".into());
    configure(&mut fake);
    FakePlatform(Arc::new(fake))
}

#[test]
fn a_paste_writes_clicks_v_and_restores_the_previous_clipboard() {
    let p = platform(|_| {});
    let restore = insert_with(&p, "hello", None, NONE, NONE).unwrap();
    restore
        .expect("a saved clipboard is restored")
        .join()
        .unwrap();
    let keys = p.0.keys.lock().unwrap();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0].1, "Press");
    assert_eq!(keys[1], ("Unicode('v')".to_string(), "Click".to_string()));
    assert_eq!(keys[2].1, "Release");
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("previous"));
}

#[test]
fn nothing_is_restored_when_the_clipboard_was_empty() {
    let p = platform(|f| *f.clipboard.get_mut().unwrap() = None);
    assert!(
        insert_with(&p, "hello", None, NONE, NONE)
            .unwrap()
            .is_none()
    );
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("hello"));
}

#[test]
fn an_unreachable_clipboard_is_an_error() {
    let p = platform(|f| f.fail_open_at = Some(0));
    let err = insert_with(&p, "hello", None, NONE, NONE).unwrap_err();
    assert!(err.starts_with("failed to access clipboard"), "{err}");
}

#[test]
fn a_refused_clipboard_write_is_an_error() {
    let p = platform(|f| f.fail_write_at = Some(0));
    let err = insert_with(&p, "hello", None, NONE, NONE).unwrap_err();
    assert!(err.starts_with("failed to write text"), "{err}");
}

#[test]
fn an_unreachable_keyboard_is_an_error() {
    let p = platform(|f| f.fail_keyboard = true);
    let err = insert_with(&p, "hello", None, NONE, NONE).unwrap_err();
    assert!(err.starts_with("failed to create enigo"), "{err}");
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("previous"));
}

#[test]
fn a_failed_insertion_does_not_clear_clipboard_when_saved_text_is_unavailable() {
    let p = platform(|f| {
        *f.clipboard.get_mut().unwrap() = None;
        f.fail_keyboard = true;
    });

    let err = insert_with(&p, "hello", None, NONE, NONE).unwrap_err();

    assert!(err.starts_with("failed to create enigo"), "{err}");
    assert_eq!(
        *p.0.writes.lock().unwrap(),
        1,
        "only the paste text is written"
    );
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("hello"));
}

#[test]
fn each_keystroke_failure_names_its_step() {
    for (at, expected) in [
        (0, "failed to press modifier"),
        (1, "failed to press 'v'"),
        (2, "failed to release modifier"),
    ] {
        let p = platform(|f| f.fail_key_at = Some(at));
        let err = insert_with(&p, "hello", None, NONE, NONE).unwrap_err();
        assert!(err.starts_with(expected), "{err}");
        assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("previous"));
    }
}

#[test]
fn a_target_change_during_clipboard_settle_aborts_before_any_keypress() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let p = platform(|_| {});
    let target = FocusTarget {
        app: Some("Editor".into()),
        role: Some("AXTextArea".into()),
        bounds: Some(tinycomputer_bus::accessibility::ElementBounds {
            x: 1,
            y: 2,
            width: 30,
            height: 40,
        }),
    };
    let checks = AtomicUsize::new(0);
    let error = insert_with_target(&p, "authorized", &target, NONE, NONE, |seen| {
        assert_eq!(seen.app.as_deref(), Some("Editor"));
        assert_eq!(seen.role.as_deref(), Some("AXTextArea"));
        assert_eq!(seen.bounds, target.bounds);
        if checks.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err("focused element moved".into())
        }
    })
    .unwrap_err();

    assert!(error.starts_with("focus changed before paste"), "{error}");
    assert!(p.0.keys.lock().unwrap().is_empty());
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("previous"));
    assert_eq!(checks.load(Ordering::SeqCst), 2);
}

#[test]
fn a_clipboard_that_cannot_reopen_for_restore_does_not_fail_the_paste() {
    let p = platform(|f| f.fail_open_at = Some(1));
    let restore = insert_with(&p, "hello", None, NONE, NONE).unwrap().unwrap();
    restore.join().unwrap();
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("hello"));
}

#[test]
fn a_failed_restore_write_is_only_logged() {
    let p = platform(|f| f.fail_write_at = Some(1));
    let restore = insert_with(&p, "hello", None, NONE, NONE).unwrap().unwrap();
    restore.join().unwrap();
    assert_eq!(p.0.clipboard.lock().unwrap().as_deref(), Some("hello"));
}

#[test]
#[cfg(target_os = "linux")]
fn the_system_platform_reports_a_missing_display_instead_of_panicking() {
    // This checks the Linux display-server error path. A macOS CI runner has a
    // usable desktop session even when DISPLAY and WAYLAND_DISPLAY are unset.
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        assert!(insert_text("x", None).is_err());
    }
}

// ── AppleScript string escaping (macOS-only) ─────────────────

#[cfg(target_os = "macos")]
#[test]
fn escape_applescript_string_escapes_backslash_and_quote() {
    assert_eq!(escape_applescript_string("plain"), "plain");
    assert_eq!(escape_applescript_string(r#"a"b"#), r#"a\"b"#);
    assert_eq!(escape_applescript_string(r"a\b"), r"a\\b");
    // Backslash must be escaped BEFORE quotes so the order of
    // substitutions doesn't double-escape already-escaped quotes.
    assert_eq!(escape_applescript_string(r#"\"mix"#), r#"\\\"mix"#);
}

#[cfg(target_os = "macos")]
#[test]
fn escape_applescript_string_is_idempotent_on_benign_input() {
    for s in ["", "App Name", "Safari", "Sub-App 2", "123"] {
        assert_eq!(escape_applescript_string(s), s);
    }
}

// ── Focus-restore error path (macOS-only) ────────────────────

#[cfg(target_os = "macos")]
#[test]
fn restore_focus_to_app_errors_on_bogus_app_name() {
    // `osascript` returns a non-zero exit when the target app
    // cannot be activated, so we expect the helper to surface
    // that as an Err. This exercises the error-formatting branch.
    let err = restore_focus_to_app("__definitely_no_such_app_abcxyz__")
        .expect_err("bogus app should not activate");
    assert!(
        err.contains("failed to restore focus"),
        "expected focus-restore prefix in error, got: {err}"
    );
}
