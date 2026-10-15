//! Placing deep controls in a snapshot's tree: on the line that shows their
//! words in their place, or in a group after the tree, which a cut never
//! drops.

use tinycomputer_bus::browser::{ElementRef, Snapshot};

use crate::deep::{Control, Kind, Spot, place};

/// A control whose words the page shows `of` times, its own the `seen`th.
fn nth(number: u64, text: &str, seen: usize, of: usize) -> Control {
    Control {
        number,
        kind: Kind::Clickable,
        text: text.to_owned(),
        spot: Some(Spot { seen, of }),
    }
}

/// A control the only one on the page to show its words.
fn clickable(number: u64, text: &str) -> Control {
    nth(number, text, 0, 1)
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
        placed(tree, &[nth(1, "Mini", 0, 2)]),
        "- generic\n  - StaticText \"Prime\"\n  - clickable \"Mini\" [ref=d1]\n- StaticText \"Mini\""
    );
}

#[test]
fn words_shown_twice_go_to_the_line_in_the_controls_place() {
    let tree = "- StaticText \"Mini\"\n- StaticText \"Prime\"\n- StaticText \"Mini\"";
    // The page's second "Mini" is the control: the second line showing it.
    assert_eq!(
        placed(tree, &[clickable(1, "Prime"), nth(2, "Mini", 1, 2)]),
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
            spot: None,
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

/// Three result rows, each with its own "Select".
const ROWS: &str = "- generic\n  - StaticText \"Flight A\"\n  - StaticText \"Select\"\n\
                    - generic\n  - StaticText \"Flight B\"\n  - StaticText \"Select\"\n\
                    - generic\n  - StaticText \"Flight C\"\n  - StaticText \"Select\"";

#[test]
fn a_row_the_page_does_not_list_moves_no_control_to_another_row() {
    // Flight A is sold out: its "Select" shows, but takes no press, and is
    // not listed. B's and C's are the second and third of three.
    assert_eq!(
        placed(ROWS, &[nth(1, "Select", 1, 3), nth(2, "Select", 2, 3)]),
        "- generic\n  - StaticText \"Flight A\"\n  - StaticText \"Select\"\n\
         - generic\n  - StaticText \"Flight B\"\n  - clickable \"Select\" [ref=d1]\n\
         - generic\n  - StaticText \"Flight C\"\n  - clickable \"Select\" [ref=d2]"
    );
}

#[test]
fn words_the_tree_shows_another_number_of_times_go_to_the_group() {
    // The page shows "Select" four times, the tree three: which line is
    // whose cannot be told, so none is guessed.
    let group = "- group \"Controls inside web components\"\n  - clickable \"Select\" [ref=d1]";
    assert_eq!(
        placed(ROWS, &[nth(1, "Select", 1, 4)]),
        format!("{ROWS}\n{group}")
    );
    assert_eq!(
        placed(ROWS, &[nth(1, "Select", 0, 2)]),
        format!("{ROWS}\n{group}")
    );
}

#[test]
fn words_compare_in_lower_case_as_the_tree_shows_them_drawn() {
    assert_eq!(
        placed("- StaticText \"LOG IN\"", &[clickable(1, "Log in")]),
        "- clickable \"Log in\" [ref=d1]"
    );
}

#[test]
fn a_line_already_taken_is_never_taken_twice() {
    assert_eq!(
        placed(
            "- StaticText \"Mini\"",
            &[clickable(1, "Mini"), clickable(2, "Mini")]
        ),
        "- clickable \"Mini\" [ref=d1]\n- group \"Controls inside web components\"\n  - clickable \"Mini\" [ref=d2]"
    );
}
