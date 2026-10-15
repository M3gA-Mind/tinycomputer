//! Placing deep controls in a snapshot's tree.
//!
//! agent-browser shows the words of a shadow root's control as plain text:
//! `- StaticText "LOG IN"`, or a row's pieces under an unnamed container
//! (`- generic`). A line shows a control's words when its whole text is
//! those words: a text line, or a container holding only text and other
//! such containers (no ref, no name of its own). Words compare without
//! their spaces, since the tree splits a row's text into pieces and drops
//! the spaces between them, and in lower case, as the tree shows text as
//! it is drawn.
//!
//! The same words often show more than once ("Select" on every result),
//! and not every place is a listed control: a sold-out row, a heading, a
//! button hidden until hovered. So the look counts where each control's
//! words show among the page's text, as the tree shows its lines
//! ([`Spot`]), and a control goes on that place among the outermost lines
//! showing its words, the innermost line there, only when the tree shows
//! them as many times as the page does. That line becomes the control at
//! the same depth, `- clickable "LOG IN" [ref=d1]`, and keeps what it holds
//! below it.
//!
//! A control that shows no words, or whose place cannot be told for
//! certain (a compact snapshot drops the text after its last ref), is
//! listed after the tree in a group of its own. That group is the one place
//! those controls show, so when the snapshot is cut to its size it is kept
//! whole and the tree before it is cut shorter.

use std::collections::HashMap;

use tinycomputer_bus::browser::{ElementRef, Snapshot};

use super::Control;
#[cfg(doc)]
use super::Spot;

/// The line the controls no line holds are listed under.
const GROUP: &str = "- group \"Controls inside web components\"";

/// What agent-browser puts in place of a tree with nothing in it.
const PLACEHOLDERS: &[&str] = &["(empty page)", "(no interactive elements)"];

/// One line of the tree, as placing reads it.
struct Row {
    depth: usize,
    shape: Shape,
}

enum Shape {
    /// A text line, its words as [`key`] makes them.
    Text(String),
    /// A line with no ref, no name, and no value: it shows only what it holds.
    Container,
    /// Anything else: a control, or a line with a name of its own.
    Fixed,
}

fn row(line: &str) -> Row {
    let body = line.trim_start_matches(' ');
    let depth = (line.len() - body.len()) / 2;
    let text = body
        .strip_prefix("- StaticText ")
        .and_then(|quoted| serde_json::from_str::<String>(quoted).ok());
    let shape = match text {
        Some(text) => Shape::Text(key(&text)),
        None if body.starts_with("- ")
            && !body.contains("ref=")
            && !body.contains(" \"")
            && !body.contains(':') =>
        {
            Shape::Container
        }
        None => Shape::Fixed,
    };
    Row { depth, shape }
}

/// Words as they are compared: without their spaces, in lower case.
fn key(text: &str) -> String {
    text.split_whitespace().collect::<String>().to_lowercase()
}

/// Whether the whole text of the line at `at` is `words`.
fn shows(rows: &[Row], at: usize, words: &str) -> bool {
    match &rows[at].shape {
        Shape::Text(text) => text == words,
        Shape::Fixed => false,
        Shape::Container => {
            let mut held = String::new();
            for row in rows[at + 1..]
                .iter()
                .take_while(|row| row.depth > rows[at].depth)
            {
                match &row.shape {
                    Shape::Fixed => return false,
                    Shape::Container => {}
                    Shape::Text(text) => {
                        held.push_str(text);
                        if !words.starts_with(&held) {
                            return false;
                        }
                    }
                }
            }
            !held.is_empty() && held == words
        }
    }
}

/// The index just past what the line at `at` holds.
fn end_of(rows: &[Row], at: usize) -> usize {
    at + 1
        + rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > rows[at].depth)
            .count()
}

/// The places the tree shows `words`, in its order: of each outermost line
/// showing them, the innermost line among its own that does.
fn places(rows: &[Row], words: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < rows.len() {
        if shows(rows, at, words) {
            let end = end_of(rows, at);
            found.push(
                (at..end)
                    .rev()
                    .find(|&line| shows(rows, line, words))
                    .unwrap_or(at),
            );
            at = end;
        } else {
            at += 1;
        }
    }
    found
}

/// `tree` with `controls` placed in it, as the tree itself and the group of
/// those no line holds for certain; either may be empty.
fn merge(tree: &str, controls: &[Control]) -> (String, String) {
    let lines: Vec<&str> = tree.lines().collect();
    let rows: Vec<Row> = lines.iter().map(|line| row(line)).collect();
    let mut placed: Vec<Option<&Control>> = vec![None; rows.len()];
    let mut shown: HashMap<String, Vec<usize>> = HashMap::new();
    let mut left = Vec::new();
    for control in controls {
        let words = key(&control.text);
        let line = control.spot.filter(|_| !words.is_empty()).and_then(|spot| {
            let found = shown
                .entry(words.clone())
                .or_insert_with(|| places(&rows, &words));
            (found.len() == spot.of)
                .then(|| found[spot.seen])
                .filter(|&line| placed[line].is_none())
        });
        match line {
            Some(line) => placed[line] = Some(control),
            None => left.push(control),
        }
    }
    let body = if PLACEHOLDERS.contains(&tree.trim()) {
        String::new()
    } else {
        lines
            .iter()
            .zip(&rows)
            .zip(&placed)
            .map(|((line, row), placed)| {
                placed.map_or_else(|| (*line).to_owned(), |control| control.line(row.depth))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let group = if left.is_empty() {
        String::new()
    } else {
        std::iter::once(GROUP.to_owned())
            .chain(left.iter().map(|control| control.line(1)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    (body, group)
}

/// Puts `controls` in `snapshot`, whose tree agent-browser gave as `tree`
/// before it was cut to `max_chars`, and lists their refs after
/// agent-browser's. With no controls, nothing changes.
pub(crate) fn place(snapshot: &mut Snapshot, tree: &str, controls: &[Control], max_chars: usize) {
    if controls.is_empty() {
        return;
    }
    let (body, group) = merge(tree, controls);
    let whole = match (body.is_empty(), group.is_empty()) {
        (true, _) => group.clone(),
        (_, true) => body.clone(),
        _ => format!("{body}\n{group}"),
    };
    let group_size = group.chars().count();
    snapshot.truncated = whole.chars().count() > max_chars;
    snapshot.tree = if snapshot.truncated {
        if body.is_empty() || group.is_empty() || group_size + 1 >= max_chars {
            whole.chars().take(max_chars).collect()
        } else {
            let mut cut: String = body.chars().take(max_chars - group_size - 1).collect();
            cut.push('\n');
            cut.push_str(&group);
            cut
        }
    } else {
        whole
    };
    snapshot
        .refs
        .extend(controls.iter().map(|control| ElementRef {
            id: control.reference(),
            role: control.kind.as_str().to_owned(),
            name: control.text.clone(),
        }));
}
