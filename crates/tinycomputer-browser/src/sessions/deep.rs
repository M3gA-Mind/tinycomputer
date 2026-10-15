//! A session's deep controls (`crate::deep`): the look that follows each
//! snapshot, and actions on the `d` refs it hands out.
//!
//! agent-browser cannot reach a deep control, so the session acts on it by
//! position, as a person's hand does. The element is found by its mark
//! through the open shadow roots; gone, its ref is stale, so the caller takes
//! a fresh snapshot. It is brought into the window, and the middle of what
//! the window shows of it is checked to be what a press there reaches: the
//! element, or something inside it, through open shadow roots. Then the
//! pointer moves there and presses with the browser's own input events
//! (`convert::press_at`, as the surface presses a point), never a script's
//! `click()`, which a page can tell from a person's and which sends no
//! pointer down.
//!
//! Keys go only where the action points, as `focus.rs` keeps them: before a
//! focus, a key, or text, the focus comes off whatever held it, the element
//! takes it (pressed, for text, as a person clicks into a field), and the
//! action is refused unless the focus is on the element or inside it. Text
//! goes only into what takes it: a field or an editable region.

use std::time::Duration;

use serde_json::{Value, json};
use tinycomputer_bus::browser::Action;

use super::Session;
use super::focus::missed;
use crate::convert;
use crate::deep::{self, Control};
use crate::error::{Error, Result};

/// The longest the look after a snapshot may take: past it, the snapshot is
/// handed over as agent-browser gave it.
const LOOK: Duration = Duration::from_secs(3);

impl Session {
    /// Looks for the deep controls under `scope`, a CSS selector, or all the
    /// page when `None`, after a snapshot, and hands out their refs. Never
    /// fails: a look that cannot run, or takes longer than [`LOOK`], hands
    /// out none, and the refs of the snapshot before are stale.
    pub(super) async fn find_deep(&mut self, scope: Option<&str>) -> Vec<Control> {
        let script = self.deep.look(scope);
        let looking = self.run(json!({"action": "evaluate", "script": script}));
        let reply = tokio::time::timeout(LOOK, looking)
            .await
            .ok()
            .and_then(Result::ok);
        self.deep
            .take(reply.as_ref().and_then(|data| data.get("result")))
    }

    /// Performs `action`, one [`deep::supports`] admits, on deep control
    /// `number`, and returns what it read (`null` for an action that reads
    /// nothing).
    ///
    /// # Errors
    ///
    /// [`Error::StaleRef`] when the latest snapshot did not hand the ref out
    /// or its element is gone, [`Error::NotActionable`] when it shows nowhere
    /// in the window, something else lies over its middle, or it takes no
    /// text meant for it, [`Error::FocusMissed`] when the focus does not land
    /// on it, and whatever the engine reports.
    pub(super) async fn act_deep(&mut self, number: u64, action: &Action) -> Result<Value> {
        if !self.deep.holds(number) {
            return Err(stale(number));
        }
        match action {
            Action::Click { .. } => {
                let (x, y) = self.aim(number, false).await?;
                self.press_at(x, y).await?;
            }
            Action::Hover { .. } => {
                let (x, y) = self.aim(number, false).await?;
                self.run(convert::move_to(x, y)).await?;
            }
            Action::Focus { .. } => self.focus_deep(number).await?,
            Action::Press { key, .. } => {
                self.focus_deep(number).await?;
                self.run(json!({"action": "press", "key": key})).await?;
            }
            Action::Type { text, delay_ms, .. } => {
                self.press_into(number, "end").await?;
                self.type_keys(text, *delay_ms).await?;
            }
            Action::Fill { value, .. } => {
                self.press_into(number, "select").await?;
                // The text replaces what the field held, selected; no text
                // deletes it.
                let command = if value.is_empty() {
                    json!({"action": "press", "key": "Backspace"})
                } else {
                    json!({"action": "inserttext", "text": value})
                };
                self.run(command).await?;
            }
            Action::GetText { .. } => {
                let read = self.find_mark(number, "text").await?;
                return Ok(read.get("text").cloned().unwrap_or(Value::Null));
            }
            Action::IsVisible { .. } => {
                let read = self.find_mark(number, "visible").await?;
                return Ok(read.get("visible").cloned().unwrap_or(Value::Bool(false)));
            }
            other => return Err(deep::unsupported(other)),
        }
        Ok(Value::Null)
    }

    /// Runs `op` (`deep.js`) on the element marked `number`: its reply, or
    /// `None` when no one element carries the mark.
    async fn probe(&mut self, number: u64, op: &str) -> Result<Option<Value>> {
        self.probe_for(number, op, false).await
    }

    /// [`Session::probe`], asking an aim whether the element takes text
    /// when `field`.
    async fn probe_for(&mut self, number: u64, op: &str, field: bool) -> Result<Option<Value>> {
        let data = self
            .run(json!({"action": "evaluate", "script": deep::act(op, number, field)}))
            .await?;
        let reply = data.get("result").cloned().unwrap_or(Value::Null);
        Ok((reply.is_object() && reply.get("gone") != Some(&Value::Bool(true))).then_some(reply))
    }

    /// [`Session::probe`], with an element gone as a stale ref: for a probe
    /// before anything was sent to the element.
    async fn find_mark(&mut self, number: u64, op: &str) -> Result<Value> {
        self.probe(number, op).await?.ok_or_else(|| stale(number))
    }

    /// Where a press on deep control `number` goes: the middle of what the
    /// window shows of it, brought into the window first, once that point is
    /// checked to reach the element. For `text`, only an element that takes
    /// text or holds something that does is pressed at all.
    async fn aim(&mut self, number: u64, text: bool) -> Result<(f64, f64)> {
        let aimed = self
            .probe_for(number, "aim", text)
            .await?
            .ok_or_else(|| stale(number))?;
        let reference = deep::reference(number);
        if aimed.get("unseen") == Some(&Value::Bool(true)) {
            return Err(Error::not_actionable(format!(
                "@{reference} shows nowhere in the window: it draws no box, or the page hides it"
            )));
        }
        let (Some(x), Some(y)) = (
            aimed.get("x").and_then(Value::as_f64),
            aimed.get("y").and_then(Value::as_f64),
        ) else {
            return Err(Error::not_actionable(format!(
                "@{reference} could not be measured"
            )));
        };
        if let Some(cover) = aimed.get("cover").and_then(Value::as_str) {
            // Named by tag, id, and class, as agent-browser names a cover:
            // nothing more of the page's own text goes into the error.
            let cover: String = cover
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || "-_#.".contains(*character)
                })
                .take(80)
                .collect();
            let cover = if cover.is_empty() {
                "an element"
            } else {
                &cover
            };
            return Err(Error::not_actionable(format!(
                "@{reference} is covered by <{cover}> at its middle, so a press would land on that instead; dismiss or close what covers it first"
            )));
        }
        if text && aimed.get("field") != Some(&Value::Bool(true)) {
            return Err(no_text(
                &reference,
                "there is no field or editable region in it",
            ));
        }
        Ok((x, y))
    }

    /// Presses the window at (`x`, `y`) as a person's pointer does.
    async fn press_at(&mut self, x: f64, y: f64) -> Result<()> {
        for command in convert::press_at(x, y) {
            self.run(command).await?;
        }
        Ok(())
    }

    /// Gives deep control `number` the focus, and only it.
    async fn focus_deep(&mut self, number: u64) -> Result<()> {
        self.blur().await?;
        let found = self.find_mark(number, "focus").await?;
        on_it(&found)
    }

    /// Presses deep control `number` to put the focus in it, as a person
    /// clicks into a field, then readies the field it focused for text:
    /// with its caret after what it holds (`end`) or all it holds selected
    /// (`select`).
    async fn press_into(&mut self, number: u64, how: &str) -> Result<()> {
        // Refused before the focus moves: what held it (a box whose
        // suggestions are open, say) keeps it.
        self.aim(number, true).await?;
        self.blur().await?;
        // Measured again: letting go of the focus can close what it held open.
        let (x, y) = self.aim(number, true).await?;
        self.press_at(x, y).await?;
        // The press reached the page: an element gone now went with it, which
        // is no ref to retry blindly.
        let reference = deep::reference(number);
        let Some(found) = self.probe(number, how).await? else {
            return Err(Error::not_actionable(format!(
                "@{reference} went away when it was pressed, so no text was sent"
            )));
        };
        on_it(&found)?;
        if found.get("takes") == Some(&Value::Bool(true)) {
            Ok(())
        } else {
            Err(no_text(
                &reference,
                "pressing it focused no field or editable region in it",
            ))
        }
    }

    /// Types `text` into the focused element a character at a time, as
    /// agent-browser types into a field, `delay_ms` apart.
    async fn type_keys(&mut self, text: &str, delay_ms: Option<u64>) -> Result<()> {
        let Some(delay) = delay_ms.filter(|delay| *delay > 0) else {
            return self
                .run(json!({"action": "keyboard", "subaction": "type", "text": text}))
                .await
                .map(drop);
        };
        for (index, character) in text.chars().enumerate() {
            if index > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            self.run(
                json!({"action": "keyboard", "subaction": "type", "text": character.to_string()}),
            )
            .await?;
        }
        Ok(())
    }
}

/// Whether the focus, as a probe found it, is on the element or inside it.
fn on_it(found: &Value) -> Result<()> {
    if found.get("inside") == Some(&Value::Bool(true)) {
        Ok(())
    } else {
        Err(missed(found))
    }
}

/// The refusal of text for `reference`, which takes none, and why.
fn no_text(reference: &str, why: &str) -> Error {
    Error::not_actionable(format!(
        "@{reference} takes no typed text: {why}, so no text was sent"
    ))
}

fn stale(number: u64) -> Error {
    Error::StaleRef {
        reference: deep::reference(number),
    }
}
