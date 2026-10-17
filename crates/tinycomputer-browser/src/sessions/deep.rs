//! A session's deep controls (`crate::deep`): the look that follows each
//! snapshot, and actions on the `d` refs it hands out.
//!
//! agent-browser cannot reach a deep control, so the session acts on it by
//! position, as a person's hand does. The element is found by its mark
//! through the open shadow roots, and must still show the words its ref was
//! handed out with; gone, or showing another item, its ref is stale, so the
//! caller takes a fresh snapshot. It is brought into the window, and once it
//! has held still for two frames, a point on it is chosen where what a
//! press reaches is the element, or something inside it that is no control
//! of its own. Then the pointer moves there and presses with the browser's
//! own input events (`convert::press_at`, as the surface presses a point),
//! never a script's `click()`, which a page can tell from a person's and
//! which sends no pointer down. Between the aim and the press the page can
//! still move: a guard set with the aim stops any part of the press whose
//! path misses the element, and the press is reported refused.
//!
//! Keys go only where the action points, as `focus.rs` keeps them: before a
//! focus, a key, or text, the focus comes off whatever held it, the element
//! takes it (pressed, for text, as a person clicks into a field), and the
//! action is refused unless the focus, two frames later, is on the element
//! or inside it. Text goes only into what takes it: a field or an editable
//! region.

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

/// Whether an aim readies a press, and for what.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Aim {
    /// The pointer only moves there.
    Hover,
    /// A press, guarded.
    Press,
    /// A check, before the focus comes off anything, that the element takes
    /// text: nothing is pressed yet.
    Text,
    /// A press into a field, guarded.
    Type,
}

impl Session {
    /// Looks for the deep controls under `scope`, a CSS selector, or all the
    /// page when `None`, after a snapshot, and hands out their refs. Never
    /// fails: a look that cannot run, or takes longer than [`LOOK`], hands
    /// out none, and the refs of the snapshot before are stale. While
    /// agent-browser reads a frame, there is no look.
    pub(super) async fn find_deep(&mut self, scope: Option<&str>) -> Vec<Control> {
        if !self.deep.looks() {
            self.deep.skip();
            return Vec::new();
        }
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
    /// or its element is gone or shows something else, [`Error::NotActionable`]
    /// when it shows nowhere in the window, will not hold still, is covered,
    /// holds another control where it shows, a press would have reached
    /// something else, or it takes no text meant for it,
    /// [`Error::FocusMissed`] when the focus does not land on it, and
    /// whatever the engine reports.
    pub(super) async fn act_deep(&mut self, number: u64, action: &Action) -> Result<Value> {
        let Some(words) = self.deep.words(number).map(str::to_owned) else {
            return Err(stale(number));
        };
        match action {
            Action::Click { .. } => {
                let (x, y) = self.aim(number, &words, Aim::Press).await?;
                self.press_at(x, y).await?;
                self.pressed(number).await?;
            }
            Action::Hover { .. } => {
                let (x, y) = self.aim(number, &words, Aim::Hover).await?;
                self.run(convert::move_to(x, y)).await?;
            }
            Action::Focus { .. } => self.focus_deep(number, &words).await?,
            Action::Press { key, .. } => {
                self.focus_deep(number, &words).await?;
                self.run(json!({"action": "press", "key": key})).await?;
            }
            Action::Type { text, delay_ms, .. } => {
                let found = self.press_into(number, &words, "end").await?;
                // A field with no caret to set (`email`, `number`) is typed
                // into after what it holds all the same.
                if found.get("caret") == Some(&Value::Bool(false)) {
                    self.run(json!({"action": "press", "key": "End"})).await?;
                }
                self.type_keys(text, *delay_ms).await?;
            }
            Action::Fill { value, .. } => {
                self.press_into(number, &words, "select").await?;
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
                let read = self.find_mark(number, "text", Some(&words)).await?;
                return Ok(read.get("text").cloned().unwrap_or(Value::Null));
            }
            Action::IsVisible { .. } => {
                let read = self.find_mark(number, "visible", None).await?;
                return Ok(read.get("visible").cloned().unwrap_or(Value::Bool(false)));
            }
            other => return Err(deep::unsupported(other)),
        }
        Ok(Value::Null)
    }

    /// Runs `script`, a probe of the element marked `number` (`deep.js`):
    /// its reply, or `None` when no one element carries the mark, or it no
    /// longer shows the words it was asked to.
    async fn probe(&mut self, script: String) -> Result<Option<Value>> {
        let data = self
            .run(json!({"action": "evaluate", "script": script}))
            .await?;
        let reply = data.get("result").cloned().unwrap_or(Value::Null);
        Ok((reply.is_object() && reply.get("gone") != Some(&Value::Bool(true))).then_some(reply))
    }

    /// `op` on the element marked `number`, which must still show `words`
    /// when they are given, with an element gone as a stale ref: for a probe
    /// before anything was sent to the element.
    async fn find_mark(&mut self, number: u64, op: &str, words: Option<&str>) -> Result<Value> {
        self.probe(deep::act(op, number, words))
            .await?
            .ok_or_else(|| stale(number))
    }

    /// Where a press on deep control `number`, which must still show
    /// `words`, goes: a point on it, brought into the window and held still,
    /// that reaches the element itself. For text, only an element that
    /// takes text or holds something that does goes ahead.
    async fn aim(&mut self, number: u64, words: &str, aim: Aim) -> Result<(f64, f64)> {
        let text = matches!(aim, Aim::Text | Aim::Type);
        let guard = matches!(aim, Aim::Press | Aim::Type);
        let aimed = self
            .probe(deep::aim(number, words, text, guard))
            .await?
            .ok_or_else(|| stale(number))?;
        let reference = deep::reference(number);
        let said = |key: &str| aimed.get(key).and_then(Value::as_str).map(named);
        if aimed.get("unseen") == Some(&Value::Bool(true)) {
            return Err(Error::not_actionable(format!(
                "@{reference} shows nowhere in the window: it draws no box, or the page hides it"
            )));
        }
        if aimed.get("moving") == Some(&Value::Bool(true)) {
            return Err(Error::not_actionable(format!(
                "@{reference} would not hold still in the window, so it was not pressed; let the page settle and retry"
            )));
        }
        if let Some(control) = said("nested") {
            return Err(Error::not_actionable(format!(
                "@{reference} shows another control everywhere a press could reach it (<{control}>), so it was not pressed; press that control by its own ref"
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
        if let Some(cover) = said("cover") {
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

    /// Whether the press on deep control `number` went where it was aimed.
    /// A press that led to another page did; one the guard stopped, because
    /// something else was on top by then, did not, and nothing of it reached
    /// the page's own handlers.
    async fn pressed(&mut self, number: u64) -> Result<()> {
        let Ok(data) = self
            .run(json!({"action": "evaluate", "script": deep::pressed()}))
            .await
        else {
            return Ok(());
        };
        let missed = data
            .get("result")
            .and_then(|result| result.get("missed"))
            .and_then(Value::as_str);
        match missed {
            Some(other) => Err(Error::not_actionable(format!(
                "@{} moved, or <{}> came over it, as it was pressed, so the press was stopped; take a fresh snapshot and retry",
                deep::reference(number),
                named(other)
            ))),
            None => Ok(()),
        }
    }

    /// Gives deep control `number`, which must still show `words`, the
    /// focus, and only it.
    async fn focus_deep(&mut self, number: u64, words: &str) -> Result<()> {
        self.blur().await?;
        let found = self.find_mark(number, "focus", Some(words)).await?;
        on_it(&found)
    }

    /// Presses deep control `number`, which must still show `words`, to put
    /// the focus in it, as a person clicks into a field, then readies the
    /// field it focused for text: with its caret after what it holds
    /// (`end`) or all it holds selected (`select`). Returns what the field
    /// said of itself.
    async fn press_into(&mut self, number: u64, words: &str, how: &str) -> Result<Value> {
        // Refused before the focus moves: what held it (a box whose
        // suggestions are open, say) keeps it.
        self.aim(number, words, Aim::Text).await?;
        self.blur().await?;
        // Aimed again: letting go of the focus can close what it held open.
        let (x, y) = self.aim(number, words, Aim::Type).await?;
        self.press_at(x, y).await?;
        self.pressed(number).await?;
        // The press reached the page: an element gone now went with it, which
        // is no ref to retry blindly.
        let reference = deep::reference(number);
        let Some(found) = self.probe(deep::act(how, number, None)).await? else {
            return Err(Error::not_actionable(format!(
                "@{reference} went away when it was pressed, so no text was sent"
            )));
        };
        on_it(&found)?;
        if found.get("takes") == Some(&Value::Bool(true)) {
            Ok(found)
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

/// An element the page described by tag, id, and class, as agent-browser
/// names a cover: nothing more of the page's own text goes into an error.
fn named(described: &str) -> String {
    let named: String = described
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "-_#.".contains(*character))
        .take(80)
        .collect();
    if named.is_empty() {
        "an element".to_owned()
    } else {
        named
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
