//! A scripted page with controls inside shadow roots, for the deep tests:
//! its snapshot, the look that finds its deep controls, and the answers its
//! marked elements give.

use serde_json::{Value, json};
use tinycomputer_bus::browser::{Action, Target};

use crate::fake::{Fake, ok};

/// The `deep.js` operation `command` runs, if it runs one.
pub(super) fn op(command: &Value) -> Option<String> {
    let script = command["script"].as_str()?;
    if command["action"] != "evaluate" || !script.contains("data-tc-deep") {
        return None;
    }
    let call = script.rfind(")(\"")? + 3;
    let end = call + script[call..].find('"')?;
    Some(script[call..end].to_owned())
}

/// The ops run, in order.
pub(super) fn ops(fake: &Fake) -> Vec<String> {
    fake.sent().iter().filter_map(op).collect()
}

/// The options each `op` was run with, in order.
pub(super) fn options(fake: &Fake, op_named: &str) -> Vec<Value> {
    fake.sent()
        .iter()
        .filter(|command| op(command).as_deref() == Some(op_named))
        .map(|command| {
            let script = command["script"].as_str().unwrap();
            let at = script.rfind(&format!(")(\"{op_named}\", ")).unwrap() + op_named.len() + 5;
            serde_json::from_str(&script[at..script.len() - 1]).unwrap()
        })
        .collect()
}

/// The mouse events sent, as `(event, x, y)`.
pub(super) fn mouse(fake: &Fake) -> Vec<(String, f64, f64)> {
    fake.sent()
        .iter()
        .filter(|command| command["action"] == "mouse")
        .map(|command| {
            (
                command["eventType"].as_str().unwrap().to_owned(),
                command["x"].as_f64().unwrap(),
                command["y"].as_f64().unwrap(),
            )
        })
        .collect()
}

pub(super) const TREE: &str = "- banner\n  - StaticText \"LOG IN\"\n- button \"Book\" [ref=e1]";

/// A page whose snapshot is [`TREE`] and whose look finds "LOG IN", the
/// only words of its kind on the page, and a nameless row as `d1` and `d2`;
/// every other op answers `answer(op)`.
pub(super) fn page(answer: impl Fn(&str) -> Value + Send + Sync + 'static) -> Fake {
    Fake::scripted(move |command| {
        if command["action"] == "snapshot" {
            return Some(ok(&json!({
                "snapshot": TREE,
                "refs": {"e1": {"role": "button", "name": "Book"}},
            })));
        }
        let op = op(command)?;
        let result = if op == "find" {
            json!({"found": [
                {"n": 1, "kind": "clickable", "text": "LOG IN", "seen": 0, "of": 1},
                {"n": 2, "kind": "clickable", "text": ""},
            ], "next": 3})
        } else {
            answer(&op)
        };
        Some(ok(&json!({ "result": result })))
    })
}

/// The answers of a page whose deep elements show still at (40, 12), take
/// the focus, take text, and take every press aimed at them.
pub(super) fn shown(op: &str) -> Value {
    match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "field": true}),
        "pressed" => json!({"missed": null}),
        "focus" | "end" | "select" => json!({"inside": true, "on": "div", "takes": true}),
        "text" => json!({"text": "LOG IN"}),
        "visible" => json!({"visible": true}),
        _ => json!({"gone": true}),
    }
}

pub(super) fn click(reference: &str) -> Action {
    Action::Click {
        target: Target::reference(reference),
        new_tab: false,
    }
}
