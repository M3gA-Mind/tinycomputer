//! Placing deep controls in a snapshot's tree: on the line that shows their
//! words, or in a group after the tree, which a cut never drops.

use tinycomputer_bus::browser::{ElementRef, Snapshot};

use crate::deep::{Control, Kind, place};

fn clickable(number: u64, text: &str) -> Control {
    Control {
        number,
        kind: Kind::Clickable,
        text: text.to_owned(),
    }
}

/// A snapshot of `tree` as agent-browser gave it, with one ref of its own.
fn snapshot(tree: &str) -> Snapshot {
    Snapshot {
        url: "https://cabs.test/".to_owned(),
        title: "Cabs".to_owned(),
        sequence: 1,
        tree: tree.to_owned(),
        refs: vec![ElementRef {
            id: "e1".to_owned(),
            role: "button".to_owned(),
            name: "Book".to_owned(),
        }],
        truncated: false,
    }
}

/// `tree` with `controls` placed in it, uncut.
fn placed(tree: &str, controls: &[Control]) -> String {
    let mut shot = snapshot(tree);
    place(&mut shot, tree, controls, 200_000);
    shot.tree
}

#[test]
fn no_controls_leave_the_snapshot_as_it_was() {
    let tree = "- banner\n  - StaticText \"LOG IN\"\n- button \"Book\" [ref=e1]";
    let mut shot = snapshot(tree);
    place(&mut shot, tree, &[], 10);
    assert_eq!(shot, snapshot(tree));
}

#[test]
fn a_text_line_with_the_controls_words_becomes_the_control() {
    let tree = "- banner\n  - StaticText \"LOG IN\"\n- button \"Book\" [ref=e1]\n- StaticText \"Please log in\"";
    let mut shot = snapshot(tree);
    place(&mut shot, tree, &[clickable(1, "LOG IN")], 200_000);
    assert_eq!(
        shot.tree,
        "- banner\n  - clickable \"LOG IN\" [ref=d1]\n- button \"Book\" [ref=e1]\n- StaticText \"Please log in\""
    );
    assert_eq!(
        shot.refs.last(),
        Some(&ElementRef {
            id: "d1".to_owned(),
            role: "clickable".to_owned(),
            name: "LOG IN".to_owned(),
        })
    );
    assert_eq!(shot.refs[0].id, "e1", "agent-browser's refs come first");
    assert!(!shot.truncated);
}

#[test]
fn a_container_of_the_words_pieces_becomes_the_control_and_keeps_them() {
    // The tree drops the space between two pieces of a row's text.
    let tree =
        "- list\n  - generic\n    - StaticText \"Mini\"\n    - StaticText \"3 min\"\n    - image";
    assert_eq!(
        placed(tree, &[clickable(2, "Mini 3 min")]),
        "- list\n  - clickable \"Mini 3 min\" [ref=d2]\n    - StaticText \"Mini\"\n    - StaticText \"3 min\"\n    - image"
    );
    assert_eq!(
        placed(tree, &[clickable(2, "Mini3 min")]),
        placed(tree, &[clickable(2, "Mini 3 min")]).replace("Mini 3 min", "Mini3 min"),
        "words compare without their spaces"
    );
}

#[test]
fn a_line_with_a_name_or_a_ref_of_its_own_is_never_taken() {
    let tree = "- group \"Mini\"\n- generic\n  - StaticText \"Mini\"\n  - button \"Book\" [ref=e1]";
    // The named group and the container holding a button stay; the text
    // line itself is the control's.
    assert_eq!(
        placed(tree, &[clickable(1, "Mini")]),
        "- group \"Mini\"\n- generic\n  - clickable \"Mini\" [ref=d1]\n  - button \"Book\" [ref=e1]"
    );
    let named = "- group \"Mini\"";
    assert_eq!(
        placed(named, &[clickable(1, "Mini")]),
        "- group \"Mini\"\n- group \"Controls inside web components\"\n  - clickable \"Mini\" [ref=d1]"
    );
}

#[test]
fn a_container_showing_other_words_is_passed_over() {
    let tree =
        "- generic\n  - StaticText \"Prime\"\n  - StaticText \"Mini\"\n- StaticText \"Mini\"";
    assert_eq!(
        placed(tree, &[clickable(1, "Mini")]),
        "- generic\n  - StaticText \"Prime\"\n  - clickable \"Mini\" [ref=d1]\n- StaticText \"Mini\""
    );
}

#[test]
fn words_shown_twice_go_to_the_line_in_the_controls_place() {
    let tree = "- StaticText \"Mini\"\n- StaticText \"Prime\"\n- StaticText \"Mini\"";
    // The first control takes the first "Prime" there is; the one after it
    // can only be a "Mini" after that.
    assert_eq!(
        placed(tree, &[clickable(1, "Prime"), clickable(2, "Mini")]),
        "- StaticText \"Mini\"\n- clickable \"Prime\" [ref=d1]\n- clickable \"Mini\" [ref=d2]"
    );
}

#[test]
fn controls_no_line_shows_follow_the_tree_in_a_group() {
    // A compact tree keeps no text after its last ref.
    let tree = "- banner\n  - StaticText \"LOG IN\"\n- button \"Book\" [ref=e1]";
    let controls = [
        clickable(1, "LOG IN"),
        clickable(2, "Mini 3 min"),
        clickable(3, ""),
        Control {
            number: 4,
            kind: Kind::Editable,
            text: String::new(),
        },
    ];
    let mut shot = snapshot(tree);
    place(&mut shot, tree, &controls, 200_000);
    assert_eq!(
        shot.tree,
        "- banner\n  - clickable \"LOG IN\" [ref=d1]\n- button \"Book\" [ref=e1]\n\
         - group \"Controls inside web components\"\n  - clickable \"Mini 3 min\" [ref=d2]\n  \
         - clickable [ref=d3]\n  - editable [ref=d4]"
    );
    let ids: Vec<_> = shot
        .refs
        .iter()
        .map(|element| element.id.as_str())
        .collect();
    assert_eq!(ids, ["e1", "d1", "d2", "d3", "d4"]);
}

#[test]
fn an_empty_tree_is_replaced_by_the_group() {
    for empty in ["(empty page)", "(no interactive elements)", ""] {
        assert_eq!(
            placed(empty, &[clickable(1, "Mini")]),
            "- group \"Controls inside web components\"\n  - clickable \"Mini\" [ref=d1]",
            "{empty:?}"
        );
    }
}

#[test]
fn a_cut_keeps_the_group_whole_and_cuts_the_tree_shorter() {
    let tree = format!(
        "- button \"Book\" [ref=e1]\n{}",
        "- StaticText \"filler\"\n".repeat(40)
    );
    let tree = tree.trim_end();
    let controls = [clickable(1, "Mini 3 min")];
    let group = "- group \"Controls inside web components\"\n  - clickable \"Mini 3 min\" [ref=d1]";
    let mut shot = snapshot(tree);
    place(&mut shot, tree, &controls, 200);
    assert!(shot.truncated);
    assert_eq!(shot.tree.chars().count(), 200);
    assert!(shot.tree.ends_with(&format!("\n{group}")), "{}", shot.tree);
    assert!(shot.tree.starts_with("- button \"Book\" [ref=e1]"));
    // Too small for the group: cut as agent-browser's tree is.
    let mut tiny = snapshot(tree);
    place(&mut tiny, tree, &controls, 20);
    assert!(tiny.truncated);
    assert_eq!(tiny.tree, "- button \"Book\" [ref");
    // Room for all of it: whole, and not cut.
    let mut roomy = snapshot(tree);
    place(&mut roomy, tree, &controls, 10_000);
    assert!(!roomy.truncated);
    assert!(roomy.tree.ends_with(group));
}
