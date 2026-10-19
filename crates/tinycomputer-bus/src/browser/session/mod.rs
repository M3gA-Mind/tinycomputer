//! Sessions: what a host opens before it can drive a page, and what it closes
//! when it is done.
//!
//! A session is one browser the module is holding on the host's behalf — either
//! a Chrome it launched or one it attached to at an existing endpoint — plus the
//! page it is currently driving. Every other member takes a [`SessionId`],
//! because a host that runs two tasks at once must not have them fight over one
//! implicit "current page".
//!
//! Sessions are explicit rather than implicit for a second reason: a browser is
//! an expensive, long-lived, externally visible resource. Making a host name the
//! one it means is what lets the module bound how many exist, expire the ones
//! nobody is using, and report both in [`SessionInfo`].

mod types;

pub use types::{
    Screen, SessionId, SessionInfo, SessionOptions, SessionRef, SessionRequest, UserAgentBrand,
    UserAgentMetadata, Viewport,
};

#[cfg(test)]
mod session_tests;
