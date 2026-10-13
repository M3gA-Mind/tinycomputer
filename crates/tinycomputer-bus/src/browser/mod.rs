//! The browser members' vocabulary: every type they carry, and their names.
//!
//! The module serves browser automation beside desktop automation: open a
//! session, navigate, snapshot the accessibility tree, act on a ref, read the
//! page, screenshot it, close. The engine behind it is
//! [agent-browser](https://github.com/vercel-labs/agent-browser), linked as a
//! library; these types were ported from the `tinybrowser-bus` crate, which
//! this module supersedes (`docs/technical/specs/unified-agent.md`).
//!
//! Every member takes one JSON object — a [`SessionRequest`] puts the session
//! beside the member's own fields — and replies with the same
//! [`crate::DesktopResponse`] envelope as the desktop members, so a caller
//! handles a stale ref, a timeout, or a refused navigation the same way on
//! either surface.
//!
//! # What is here
//!
//! - [`names`] — one constant per browser member, plus [`names::METHODS`]
//!   listing them in dispatch order. They are served on the module's one
//!   interface, [`crate::INTERFACE`], each with a `Browser` prefix.
//! - [`session`] — opening, listing, and closing the browser a host drives.
//! - [`page`] — navigating, extracting a page as text, evaluating JavaScript.
//! - [`snapshot`] — the accessibility tree, and the refs that address it.
//! - [`action`] — every interaction, and the three ways to name an element.
//! - [`download`] — browser download events retained as waitable handles.
//! - [`output`] — screenshots, and the handle protocol that carries them.
//! - [`tool`] — the agent-facing `browser` tool: recorded parameter schema and
//!   pure argument parsing into [`Action`].
//! - [`errors`] — the failure names, the envelope code each one travels as,
//!   and which of them an agent can act on.
//!
//! The contract version is the crate's own [`crate::CONTRACT_VERSION`]: the
//! desktop and browser interfaces ship in one module and evolve together.
//!
//! # Why these types are namespaced
//!
//! Several names — [`SnapshotRequest`], [`ScreenshotRequest`] — also exist for
//! the desktop interface with different shapes. Keeping the browser vocabulary
//! under `tinycomputer_bus::browser` means neither side shadows the other.
//!
//! # Example
//!
//! Building the frame bodies for the loop an agent actually runs — open,
//! navigate, snapshot, click — without a bus in sight:
//!
//! ```
//! use tinycomputer_bus::browser::{
//!     names, Action, NavigateRequest, SessionId, SessionOptions, SessionRequest,
//!     SnapshotRequest, Target,
//! };
//!
//! let session = SessionId::new("s-1");
//!
//! let open = serde_json::to_value(SessionOptions::default())?;
//! let navigate = serde_json::to_value(SessionRequest::new(
//!     session.clone(),
//!     NavigateRequest::new("https://example.com"),
//! ))?;
//! let snapshot = serde_json::to_value(SessionRequest::new(
//!     session.clone(),
//!     SnapshotRequest::interactive(),
//! ))?;
//! let click = serde_json::to_value(SessionRequest::new(
//!     session,
//!     Action::Click { target: Target::parse("@e12"), new_tab: false },
//! ))?;
//!
//! assert_eq!(names::methods::OPEN_SESSION, "BrowserOpenSession");
//! assert_eq!(navigate["wait_until"], "load");
//! assert_eq!(snapshot["interactive_only"], true);
//! assert_eq!(click["session"], "s-1");
//! assert_eq!(click["action"], "click");
//! assert_eq!(click["target"], serde_json::json!({ "kind": "ref", "value": "e12" }));
//! # let _ = open;
//! # Ok::<(), serde_json::Error>(())
//! ```

pub mod action;
pub mod download;
pub mod errors;
pub mod names;
pub mod output;
pub mod page;
pub mod profile;
pub mod session;
pub mod snapshot;
pub mod tool;

pub use action::{Action, ActionOutcome, LocateBy, Locator, ScrollDirection, Target, WaitState};
pub use download::{DownloadId, DownloadInfo, DownloadState, DownloadWaitRequest};
pub use output::{
    ImageFormat, OutputChunk, OutputId, OutputRef, OutputRequest, ReadOutputRequest,
    ScreenshotRequest,
};
pub use page::{
    EvaluateRequest, NavigateRequest, PageState, PageText, ReadFormat, ReadRequest, WaitUntil,
};
pub use profile::{ClaimProfileRequest, ClaimProfileResponse, ProfileClaim, ReleaseProfileRequest};
pub use session::{SessionId, SessionInfo, SessionOptions, SessionRef, SessionRequest, Viewport};
pub use snapshot::{ElementRef, Snapshot, SnapshotRequest};
pub use tool::{ToolAction, ToolArgError, ToolArgs};
