//! Controls inside open shadow roots that the accessibility tree gives no
//! ref: found after every snapshot, handed out as `d` refs, and acted on by
//! position.
//!
//! Pages built from web components keep their controls inside shadow roots.
//! Chrome's accessibility tree reaches into them, and agent-browser refs
//! what has a control's role there as anywhere: a native button or field
//! inside a component gets its `eN`. But a component's own control is often
//! a plain `span` or `div` drawn with a pointer and wired to a click by the
//! component's script: no role, no tab stop, no `onclick` attribute.
//! agent-browser finds such elements by their pointer in the page's light
//! DOM only, so inside a component they come out as bare text, or, in a
//! compact snapshot, not at all. Live, a booking page built this way showed
//! its log-in link as plain text and dropped its list of results entirely,
//! and an agent had nothing to press.
//!
//! So every snapshot is followed by one look through the page's open shadow
//! roots (`deep.js`, through the engine's `evaluate`) for the elements a
//! person can press or type into that the tree holds no ref for: a pointer
//! of their own (not one inherited from a row around them), a click handler
//! set as an attribute or property, a tab stop, or an editable region. Only
//! what is drawn and not hidden from people counts, and of nested ones only
//! the outermost, though a field inside a pressable row stays a field. At
//! most [`MOST`] are kept, in the order the page draws them, each marked
//! with a `data-tc-deep` attribute holding its number. A closed shadow root
//! cannot be read and is passed over.
//!
//! Each is handed out as a ref `d1`, `d2`, …, which agent-browser's `eN`
//! cannot collide with, and placed in the snapshot's tree (`merge.rs`). A
//! number is handed out once per session, a look that went unread
//! included, and an element keeps its number from one snapshot to the next
//! only while it lives, is still listed, and still shows the words it was
//! handed out with: a node a page re-draws in place to show another item
//! gets a new one, so a ref never comes to name another element. A page
//! with nothing to list comes out exactly as agent-browser gave it. While a
//! raw `frame` command has agent-browser read a frame, there is no look:
//! the script sees only the top document.
//!
//! An action on a `d` ref is the session's own (`sessions/deep.rs`): the
//! element is found by its mark and its words, and pressed where it shows
//! with the pointer's own events.

mod merge;

pub(crate) use merge::place;

use serde_json::{Value, json};
use tinycomputer_bus::browser::{Action, Target};

use crate::error::Error;

/// The script, called as `(op, options)`.
const DEEP_JS: &str = include_str!("deep.js");

/// The most deep controls one snapshot lists.
const MOST: usize = 40;

/// The longest name a deep control is given, in characters.
const LONGEST: usize = 100;

/// What a deep control's ref starts with, before its number.
const PREFIX: char = 'd';

/// How a deep control responds, which is the role its line is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// It takes a press: a pointer of its own, or a click handler.
    Clickable,
    /// The outermost element of an editable region.
    Editable,
    /// A tab stop and nothing more.
    Focusable,
}

impl Kind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "clickable" => Some(Self::Clickable),
            "editable" => Some(Self::Editable),
            "focusable" => Some(Self::Focusable),
            _ => None,
        }
    }

    /// The role a snapshot names it by.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Clickable => "clickable",
            Self::Editable => "editable",
            Self::Focusable => "focusable",
        }
    }
}

/// One control a look found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Control {
    /// Its number: its ref is `d` and this number.
    pub(crate) number: u64,
    /// How it responds.
    pub(crate) kind: Kind,
    /// The words it shows, whitespace collapsed, at most [`LONGEST`]
    /// characters; empty when it shows none. An editable region shows its
    /// label, never what was typed into it.
    pub(crate) text: String,
    /// Where its words show among the page's text, when they show there.
    pub(crate) spot: Option<Spot>,
}

/// Where a control's words show among the page's text, counted as the tree
/// shows its lines (`merge.rs`): the `seen`th place, from 0, of `of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Spot {
    /// How many places showing the same words come before it.
    pub(crate) seen: usize,
    /// How many places show them in all.
    pub(crate) of: usize,
}

impl Control {
    /// Its ref, as a snapshot lists it.
    pub(crate) fn reference(&self) -> String {
        reference(self.number)
    }

    /// Its line in a snapshot's tree, `depth` levels in:
    /// `- clickable "LOG IN" [ref=d1]`.
    pub(crate) fn line(&self, depth: usize) -> String {
        let mut line = format!("{}- {}", "  ".repeat(depth), self.kind.as_str());
        if !self.text.is_empty() {
            line.push(' ');
            line.push_str(&Value::String(self.text.clone()).to_string());
        }
        line.push_str(" [ref=");
        line.push_str(&self.reference());
        line.push(']');
        line
    }
}

/// The ref of deep control `number`.
pub(crate) fn reference(number: u64) -> String {
    format!("{PREFIX}{number}")
}

/// The deep control `target` names, by its number: a ref `d` and digits,
/// with or without its `@`. `None` for any other target.
pub(crate) fn number(target: &Target) -> Option<u64> {
    let Target::Ref { value } = target else {
        return None;
    };
    let digits = value.trim().trim_start_matches('@').strip_prefix(PREFIX)?;
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The deep control `action` is aimed at, if its target is one.
pub(crate) fn aimed_at(action: &Action) -> Option<u64> {
    let target = match action {
        Action::Click { target, .. }
        | Action::DoubleClick { target }
        | Action::Hover { target }
        | Action::Focus { target }
        | Action::Fill { target, .. }
        | Action::Select { target, .. }
        | Action::Check { target, .. }
        | Action::GetText { target }
        | Action::GetAttribute { target, .. }
        | Action::IsVisible { target } => target,
        Action::Type { target, .. }
        | Action::Press { target, .. }
        | Action::Scroll { target, .. }
        | Action::WaitFor { target, .. } => target.as_ref()?,
        Action::Back | Action::Forward | Action::Reload => return None,
    };
    number(target)
}

/// Refuses an action a deep control cannot take, before anything reaches
/// the page: what is pressed, typed into, focused, or read takes one;
/// whatever needs agent-browser to resolve the element does not.
///
/// # Errors
///
/// [`Error::InvalidInput`] naming what a deep ref takes, or for a click in a
/// new tab or a press with no key.
pub(crate) fn supports(action: &Action) -> crate::error::Result<()> {
    match action {
        Action::Click { new_tab: true, .. } => Err(Error::invalid_input(
            "new_tab opens a link's page in a new tab, and a deep ref (dN) names no link; click it without new_tab",
        )),
        Action::Press { key, .. } if key.trim().is_empty() => {
            Err(Error::invalid_input("press needs a key"))
        }
        Action::Click { .. }
        | Action::Hover { .. }
        | Action::Focus { .. }
        | Action::Fill { .. }
        | Action::Type { .. }
        | Action::Press { .. }
        | Action::GetText { .. }
        | Action::IsVisible { .. } => Ok(()),
        other => Err(unsupported(other)),
    }
}

/// The refusal of `action` on a deep ref.
pub(crate) fn unsupported(action: &Action) -> Error {
    let verb = match action {
        Action::Click { .. } => "click",
        Action::DoubleClick { .. } => "double_click",
        Action::Hover { .. } => "hover",
        Action::Focus { .. } => "focus",
        Action::Fill { .. } => "fill",
        Action::Type { .. } => "type",
        Action::Press { .. } => "press",
        Action::Select { .. } => "select",
        Action::Check { .. } => "check",
        Action::Scroll { .. } => "scroll",
        Action::GetText { .. } => "get_text",
        Action::GetAttribute { .. } => "get_attribute",
        Action::IsVisible { .. } => "is_visible",
        Action::WaitFor { .. } => "wait_for",
        Action::Back => "back",
        Action::Forward => "forward",
        Action::Reload => "reload",
    };
    Error::invalid_input(format!(
        "{verb} does not take a deep ref (dN); a deep ref takes click, hover, focus, type, fill, press, get_text, and is_visible"
    ))
}

/// The deep refs a session's latest snapshot handed out with the words each
/// showed, the next number to mint, which no ref of the session has had,
/// and whether a raw `frame` command has agent-browser read a frame.
#[derive(Debug)]
pub(crate) struct Minted {
    live: Vec<(u64, String)>,
    next: u64,
    framed: bool,
}

impl Default for Minted {
    fn default() -> Self {
        Self {
            live: Vec::new(),
            next: 1,
            framed: false,
        }
    }
}

impl Minted {
    /// The words deep control `number` showed when the latest snapshot
    /// handed it out; `None` when it did not.
    pub(crate) fn words(&self, number: u64) -> Option<&str> {
        self.live
            .iter()
            .find(|(held, _)| *held == number)
            .map(|(_, words)| words.as_str())
    }

    /// Whether to look at all: not while agent-browser reads a frame, whose
    /// document the script, run in the top document, never sees.
    pub(crate) fn looks(&self) -> bool {
        !self.framed
    }

    /// The look for deep controls under `scope`, a CSS selector, or all
    /// the page when `None`: an element still marked with a ref this table
    /// holds keeps it while it shows the same words, and any other is given
    /// a number from `next`.
    pub(crate) fn look(&self, scope: Option<&str>) -> String {
        let keep: Vec<Value> = self
            .live
            .iter()
            .map(|(number, words)| json!({"n": number, "text": words}))
            .collect();
        call(
            "find",
            &json!({
                "scope": scope,
                "next": self.next,
                "keep": keep,
                "most": MOST,
                "longest": LONGEST,
            }),
        )
    }

    /// Takes what a look replied, `None` when its reply never came, and
    /// returns the controls it found, which are now the ones this table
    /// holds. A look whose reply is lost, or is no look's, holds none, and
    /// the numbers it may have marked elements with are never handed out:
    /// a page that comes back from the back-forward cache still carries
    /// them.
    pub(crate) fn take(&mut self, result: Option<&Value>) -> Vec<Control> {
        let Some((controls, next)) = result.and_then(|result| read(result, self.next)) else {
            self.live.clear();
            self.next = self.next.saturating_add(MOST as u64);
            return Vec::new();
        };
        self.live = controls
            .iter()
            .map(|control| (control.number, control.text.clone()))
            .collect();
        self.next = next;
        controls
    }

    /// Hands out nothing for a snapshot that had no look.
    pub(crate) fn skip(&mut self) {
        self.live.clear();
    }

    /// Follows agent-browser's choice of the frame it reads, after the raw
    /// command `action` ran: a frame once one is picked, the top document
    /// once the main frame is, or the page or tab changes.
    pub(crate) fn follow(&mut self, action: &str) {
        match action {
            "frame" => self.framed = true,
            "mainframe" | "navigate" | "tab_new" | "tab_switch" | "tab_close" => {
                self.framed = false;
            }
            _ => {}
        }
    }
}

/// `op` on the element marked `number`: `focus`, `end`, `select`, `text`, or
/// `visible` (`deep.js`). With `words`, the element must still show the
/// words its ref was handed out with.
pub(crate) fn act(op: &str, number: u64, words: Option<&str>) -> String {
    call(
        op,
        &json!({ "number": number, "words": words, "longest": LONGEST }),
    )
}

/// The aim at the element marked `number`, which must still show `words`:
/// where a press on it goes. With `field`, also whether it takes text or
/// holds something that does; with `guard`, the press that follows is
/// stopped if it would reach anything else (read by [`pressed`]).
pub(crate) fn aim(number: u64, words: &str, field: bool, guard: bool) -> String {
    call(
        "aim",
        &json!({
            "number": number,
            "words": words,
            "longest": LONGEST,
            "field": field,
            "guard": guard,
        }),
    )
}

/// Whether the press an aim guarded went where it was aimed.
pub(crate) fn pressed() -> String {
    call("pressed", &json!({}))
}

fn call(op: &str, options: &Value) -> String {
    format!(
        "({})({}, {options})",
        DEEP_JS.trim().trim_end_matches(';'),
        Value::String(op.to_owned())
    )
}

/// The controls in a look's `result`, and the next number to mint, never
/// below `next` nor any number found; `None` when `result` is not a look's
/// reply. At most [`MOST`] controls, each number once, each name at most
/// [`LONGEST`] characters.
fn read(result: &Value, next: u64) -> Option<(Vec<Control>, u64)> {
    let found = result.get("found")?.as_array()?;
    let mut controls: Vec<Control> = Vec::new();
    for item in found.iter().take(MOST) {
        let number = item.get("n").and_then(Value::as_u64).unwrap_or_default();
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .and_then(Kind::parse);
        let Some(kind) = kind.filter(|_| number > 0) else {
            continue;
        };
        if controls.iter().any(|control| control.number == number) {
            continue;
        }
        // As the page gave them: an action compares them with what the
        // element shows then, in the same words.
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(LONGEST)
            .collect();
        let index = |key: &str| {
            item.get(key)
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
        };
        let spot = index("seen")
            .zip(index("of"))
            .filter(|(seen, of)| seen < of)
            .map(|(seen, of)| Spot { seen, of });
        controls.push(Control {
            number,
            kind,
            text,
            spot,
        });
    }
    let after = controls
        .iter()
        .map(|control| control.number.saturating_add(1))
        .chain(result.get("next").and_then(Value::as_u64))
        .fold(next, u64::max);
    Some((controls, after))
}

#[cfg(test)]
mod deep_tests;
