//! Claiming a persistent browser profile folder.
//!
//! Chrome lets one browser at a time use a profile folder: a second one
//! started on it hands its window to the first and exits. A host that wants a
//! task or session to run in its persistent profile therefore claims the
//! folder first with [`methods::CLAIM_PROFILE`](crate::browser::names::methods::CLAIM_PROFILE),
//! naming how long it will wait for a Chrome that is closing. The module knows
//! how Chrome locks a folder (`SingletonLock`, or `lockfile` on Windows), waits,
//! and answers with which of three outcomes it reached. The claim stands until
//! the host gives it up with
//! [`methods::RELEASE_PROFILE`](crate::browser::names::methods::RELEASE_PROFILE)
//! or the module's grace period passes.
//!
//! Which folder to use, and whether a fresh profile is acceptable, are host
//! policy: both ride in the request.

use serde::{Deserialize, Serialize};

/// The longest wait, in milliseconds, a claim honours: a longer request is
/// clamped to it.
pub const MAX_WAIT_MS: u64 = 60_000;

/// Asks to claim a profile folder for a browser about to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimProfileRequest {
    /// The persistent profile folder.
    pub path: String,
    /// How long to wait, in milliseconds, for a Chrome holding the folder to
    /// let it go. Zero looks once. Clamped to [`MAX_WAIT_MS`].
    #[serde(default)]
    pub wait_ms: u64,
    /// Whether, when the folder stays held, the module may hand back a fresh
    /// profile folder instead of answering [`ProfileClaim::Busy`].
    #[serde(default = "default_fall_back")]
    pub fall_back_to_fresh: bool,
}

fn default_fall_back() -> bool {
    true
}

impl ClaimProfileRequest {
    /// A request for `path`, waiting `wait_ms`, falling back to a fresh
    /// profile when the folder stays held.
    #[must_use]
    pub fn new(path: impl Into<String>, wait_ms: u64) -> Self {
        Self {
            path: path.into(),
            wait_ms,
            fall_back_to_fresh: true,
        }
    }
}

/// How a claim ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileClaim {
    /// The requested folder is claimed; start the browser on it.
    Claimed,
    /// The folder stayed held and a fresh profile was not allowed.
    Busy,
    /// The folder stayed held; `path` names a fresh folder to use instead.
    FellBackToFresh,
}

/// The answer to a [`ClaimProfileRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimProfileResponse {
    /// How the claim ended.
    pub outcome: ProfileClaim,
    /// The folder to start the browser on: the requested one for
    /// [`ProfileClaim::Claimed`], a fresh one for
    /// [`ProfileClaim::FellBackToFresh`], absent for [`ProfileClaim::Busy`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Gives up a claim: the browser did not start, or holds the folder itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseProfileRequest {
    /// The folder that was claimed.
    pub path: String,
}

#[cfg(test)]
mod profile_tests;
