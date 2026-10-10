//! Injected permission/input fixture, never real devices.
#![allow(clippy::expect_used, reason = "controlled mock native operations")]
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tinycomputer_bus::accessibility::PermissionState;
#[derive(Debug)]
pub(super) struct StartGate {
    pub(super) entered: Arc<std::sync::Barrier>,
    pub(super) release: Arc<std::sync::Barrier>,
}
#[derive(Debug, Default)]
pub(super) struct Fixture {
    pub(super) reject_target: AtomicBool,
    pub(super) fail_stop: AtomicBool,
    pub(super) fail_start: AtomicBool,
    pub(super) overflow: AtomicBool,
    pub(super) restore_worker: Mutex<Option<Arc<AtomicUsize>>>,
    pub(super) start_status: Mutex<Option<GlobeHotkeyStatus>>,
    pub(super) poll_status: Mutex<Option<GlobeHotkeyStatus>>,
    pub(super) start_gate: Mutex<Option<StartGate>>,
    pub(super) calls: Mutex<Vec<&'static str>>,
}

#[derive(Debug)]
struct ProbeRestoreWorker(Arc<AtomicUsize>);

impl RestoreWorker for ProbeRestoreWorker {
    fn is_finished(&self) -> bool {
        true
    }

    fn join(&mut self) -> std::thread::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
impl Fixture {
    fn called(&self, operation: &'static str) {
        self.calls.lock().expect("calls").push(operation);
    }
    pub(super) fn status() -> GlobeHotkeyStatus {
        GlobeHotkeyStatus {
            supported: true,
            running: true,
            input_monitoring_permission: PermissionState::Granted,
            last_error: None,
            events_pending: 0,
        }
    }
}
impl Backend for Fixture {
    fn permissions(&self) -> PermissionStatus {
        self.called("permissions");
        PermissionStatus {
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::Denied,
            microphone: PermissionState::Unknown,
        }
    }
    fn request_permission(&self, _: PermissionKind) -> PermissionStatus {
        self.called("request");
        self.permissions()
    }
    fn focus(&self, _: FocusQuery) -> Result<FocusedTextContext> {
        self.called("focus");
        Ok(FocusedTextContext {
            app_name: Some("fixture".into()),
            role: None,
            text: "captured".into(),
            selected_text: None,
            raw_error: None,
            bounds: None,
        })
    }
    fn validate(&self, _: &FocusTarget) -> Result<()> {
        self.called("validate");
        if self.reject_target.load(Ordering::SeqCst) {
            Err(Error::FocusTargetChanged)
        } else {
            Ok(())
        }
    }
    fn paste(&self, _: PasteRequest) -> Result<Option<Box<dyn RestoreWorker>>> {
        self.called("paste");
        Ok(self
            .restore_worker
            .lock()
            .expect("restore worker")
            .take()
            .map(|count| Box::new(ProbeRestoreWorker(count)) as Box<dyn RestoreWorker>))
    }
    fn start(&self, _: &AtomicBool) -> Result<GlobeHotkeyStatus> {
        self.called("start");
        if let Some(gate) = self.start_gate.lock().expect("start gate").take() {
            gate.entered.wait();
            gate.release.wait();
        }
        if self.fail_start.load(Ordering::SeqCst) {
            return Err(Error::GlobeListener("controlled start failure".into()));
        }
        Ok(self
            .start_status
            .lock()
            .expect("start status")
            .take()
            .unwrap_or_else(Self::status))
    }
    fn poll(&self, _: &AtomicBool) -> Result<GlobeHotkeyPollResult> {
        self.called("poll");
        Ok(GlobeHotkeyPollResult {
            status: self
                .poll_status
                .lock()
                .expect("poll status")
                .take()
                .unwrap_or_else(Self::status),
            events: vec!["FN_DOWN".into(), "FN_UP".into()],
        })
    }
    fn read(&self, _: &AtomicBool) -> Result<(GlobeHotkeyPollResult, bool)> {
        self.called("read");
        Ok((
            GlobeHotkeyPollResult {
                status: Self::status(),
                events: vec!["FN_DOWN".into(), "FN_UP".into()],
            },
            self.overflow.load(Ordering::SeqCst),
        ))
    }
    fn stop(&self) -> Result<GlobeHotkeyStatus> {
        self.called("stop");
        if self.fail_stop.swap(false, Ordering::SeqCst) {
            return Err(Error::GlobeListener("controlled cleanup failure".into()));
        }
        let mut status = Self::status();
        status.running = false;
        Ok(status)
    }
}
