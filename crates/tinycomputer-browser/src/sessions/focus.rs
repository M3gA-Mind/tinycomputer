//! Keys go only where the caller pointed.
//!
//! The engine types and presses into whatever holds the keyboard focus: it
//! asks the target element to take the focus, and an element that takes none
//! (a heading, a label) leaves it where it was. Text typed "into" a heading
//! would then land in the field filled before it, a card number say, and a
//! key pressed on one would press whatever button last had the focus.
//!
//! So before an element is focused, filled, typed into, or sent a key, the
//! focus comes off whatever held it, the element is given it, and the focus
//! is checked to be on the element, or the action is refused
//! ([`Error::FocusMissed`]). The check marks the elements holding the focus
//! with a fresh random value, then asks the engine for that attribute on the
//! target, which resolves the target in its own frame: only the target, or a
//! shadow host around it, carries the mark. The marks come off again whatever
//! the check found.
//!
//! What the page cannot show: the focus inside a cross-origin frame (a
//! payment provider's card field). Once the focus was taken off everything,
//! it can only be in such a frame because the target took it, so that goes
//! ahead. A page's own scripts run between these steps and could fool the
//! check: it stops mistakes, not a page set on defeating it.

use std::fmt::Write as _;

use serde_json::{Value, json};

use super::Session;
use crate::error::{Error, Result};

/// The attribute marking the elements that hold the focus while it is
/// checked.
const MARK: &str = "data-tc-focus";

/// Takes the focus off whatever holds it: the deepest element reachable
/// through open shadow roots and same-origin frames, or the frame element of
/// a cross-origin frame, which takes the focus out of that frame.
const BLUR_JS: &str = r"(() => {
  let doc = document;
  let element = doc.activeElement;
  for (;;) {
    while (element && element.shadowRoot && element.shadowRoot.activeElement) {
      element = element.shadowRoot.activeElement;
    }
    if (!element || element === doc.body || element === doc.documentElement) return;
    if (element.tagName === 'IFRAME' || element.tagName === 'FRAME') {
      let inner = null;
      try { inner = element.contentDocument; } catch (e) { inner = null; }
      if (inner) { doc = inner; element = inner.activeElement; continue; }
    }
    element.blur();
    return;
  }
})()";

/// Marks the elements holding the focus with the value it is given: the
/// focused element and the open shadow hosts around it, in its own frame.
/// Answers where the focus is, `element`, `frame` (inside a cross-origin
/// frame), or `none`, with the focused element's tag name as `on`. The
/// marked elements are kept for [`UNMARK_JS`] under a key named for the value.
const MARK_JS: &str = r"((value) => {
  let doc = document;
  let element = doc.activeElement;
  let chain = [];
  for (;;) {
    while (element && element.shadowRoot && element.shadowRoot.activeElement) {
      chain.push(element);
      element = element.shadowRoot.activeElement;
    }
    if (!element || element === doc.body || element === doc.documentElement) {
      return { focus: 'none', on: 'nothing' };
    }
    if (element.tagName === 'IFRAME' || element.tagName === 'FRAME') {
      let inner = null;
      try { inner = element.contentDocument; } catch (e) { inner = null; }
      if (!inner) return { focus: 'frame', on: 'a frame' };
      doc = inner;
      element = inner.activeElement;
      chain = [];
      continue;
    }
    chain.push(element);
    for (const marked of chain) marked.setAttribute('data-tc-focus', value);
    window[Symbol.for('tinycomputer.focus.' + value)] = chain;
    return { focus: 'element', on: (element.tagName || '').toLowerCase() };
  }
})";

/// Takes the marks [`MARK_JS`] put down with the value it is given.
const UNMARK_JS: &str = r"((value) => {
  const key = Symbol.for('tinycomputer.focus.' + value);
  for (const marked of window[key] || []) {
    if (marked.getAttribute('data-tc-focus') === value) marked.removeAttribute('data-tc-focus');
  }
  delete window[key];
})";

impl Session {
    /// Takes the page's focus off whatever holds it.
    ///
    /// # Errors
    ///
    /// Whatever the engine reports.
    pub(super) async fn blur(&mut self) -> Result<()> {
        self.run(json!({"action": "evaluate", "script": BLUR_JS}))
            .await
            .map(drop)
    }

    /// Gives `selector` the focus, and only it: the focus comes off whatever
    /// held it, the element is focused, and the focus is checked to be on it.
    ///
    /// # Errors
    ///
    /// [`Error::FocusMissed`] when the focus is anywhere else, and whatever
    /// the engine reports.
    pub(super) async fn focus_only(&mut self, selector: &str) -> Result<()> {
        self.blur().await?;
        self.run(json!({"action": "focus", "selector": selector}))
            .await?;
        let value = mark_value()?;
        let found = self
            .run(json!({"action": "evaluate", "script": with_value(MARK_JS, &value)}))
            .await?;
        let checked = self.focus_is_on(selector, &value, &found["result"]).await;
        // The marks come off whatever the check found; a page that went away
        // took them with it.
        let _unmarked = self
            .run(json!({"action": "evaluate", "script": with_value(UNMARK_JS, &value)}))
            .await;
        checked
    }

    /// Whether `found`, where [`MARK_JS`] saw the focus, is on `selector`.
    async fn focus_is_on(&mut self, selector: &str, value: &str, found: &Value) -> Result<()> {
        match found["focus"].as_str() {
            Some("frame") => Ok(()),
            Some("element") => {
                let data = self
                    .run(json!({"action": "getattribute", "selector": selector, "attribute": MARK}))
                    .await?;
                if data["value"].as_str() == Some(value) {
                    Ok(())
                } else {
                    Err(missed(found))
                }
            }
            _ => Err(missed(found)),
        }
    }
}

/// The refusal for a focus that landed elsewhere, naming where by tag only:
/// anything more would be page text in an error a model reads.
pub(super) fn missed(found: &Value) -> Error {
    let on = found["on"]
        .as_str()
        .filter(|tag| {
            !tag.is_empty()
                && tag
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
        })
        .unwrap_or("nothing");
    Error::FocusMissed {
        focused: on.to_owned(),
    }
}

/// `script`, a function of one value, called with `value`.
fn with_value(script: &str, value: &str) -> String {
    format!("{script}({})", json!(value))
}

/// A fresh mark: 64 random bits, hex-encoded, which a page cannot guess.
///
/// # Errors
///
/// [`Error::ModuleFailed`] when the platform's random source is unavailable.
fn mark_value() -> Result<String> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|error| Error::failed(format!("no randomness for a focus mark: {error}")))?;
    let mut value = String::with_capacity(20);
    value.push_str("tc-");
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    Ok(value)
}
