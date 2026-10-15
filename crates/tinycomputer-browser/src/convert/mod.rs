//! Contract requests to agent-browser commands.
//!
//! agent-browser's dispatcher takes one JSON object per command, named by its
//! `action` field — the same objects its daemon receives over a socket. Every
//! function here is pure: it builds that object and nothing else, so the whole
//! mapping is testable without a browser.
//!
//! Where the contract asks for something the engine cannot express, the
//! function refuses with [`Error::InvalidInput`] naming the alternative, rather
//! than approximating it silently.

mod interaction;
mod page;
mod session;

pub(crate) use interaction::{action, press_at, selector};
pub(crate) use page::{evaluate, navigate, read, screenshot, snapshot};
pub(crate) use session::{launch, viewport};

#[cfg(doc)]
use crate::error::Error;

/// A distance that reaches either end of any page, for `top` and `bottom`.
const TO_THE_END: u32 = 1_000_000;

#[cfg(test)]
mod convert_tests;
