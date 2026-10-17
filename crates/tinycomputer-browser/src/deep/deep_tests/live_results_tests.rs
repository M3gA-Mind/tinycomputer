//! Live tests of deep controls on a page of results, gated as the other
//! live tests are (`live_tests.rs`): each control goes on its own row
//! however often its words show, a row the page re-draws in place is no
//! longer its ref's, a card is pressed beside its own button, a press
//! something comes over on its way is stopped, and text typed into a field
//! with no caret to set goes after what it holds.

#[cfg(feature = "agent-browser")]
use serde_json::json;
#[cfg(feature = "agent-browser")]
use tinycomputer_bus::browser::{Action, SnapshotRequest};

#[cfg(feature = "agent-browser")]
use super::live_tests::{click, live_page, on, page_value, ref_of};
#[cfg(feature = "agent-browser")]
use crate::error::Error;

/// A page of results built from a web component: three flights each with
/// a "Select", the first sold out (no pointer, not listed); a card whose
/// middle is its own "Info" button; a cab choice; a choice whose hover
/// raises a layer over it; and a pressable box around an `email` field.
/// Every press is recorded in `window.calls`.
#[cfg(feature = "agent-browser")]
const RESULTS: &str = r#"<!doctype html>
<html><head><style>body { margin: 0; font: 16px sans-serif; }</style></head>
<body>
<x-results></x-results>
<script>
  window.calls = [];
  const record = (what) => (event) => calls.push({ what, trusted: event.isTrusted });
  class XResults extends HTMLElement {
    constructor() {
      super();
      const root = this.attachShadow({ mode: 'open' });
      root.innerHTML = `<style>
          .row { display: flex; gap: 12px; padding: 8px; }
          .pick { cursor: pointer; }
          .off { cursor: not-allowed; color: #999; }
          .card { cursor: pointer; width: 400px; padding: 8px 0; text-align: center; }
          .layer { position: fixed; inset: 0; display: none; }
          .wrap { cursor: pointer; width: 240px; }
          .wrap input { width: 220px; }
        </style>
        <div class="row"><div>Flight A</div><div class="off">Select</div></div>
        <div class="row"><div>Flight B</div><div class="pick" data-flight="B">Select</div></div>
        <div class="row"><div>Flight C</div><div class="pick" data-flight="C">Select</div></div>
        <div class="card" id="card"><span>Mumbai</span> <button id="info">Info</button> <span>Delhi</span></div>
        <div class="row"><div class="pick" id="cab">Mini</div></div>
        <div class="row"><div class="pick" id="shy">Prime</div></div>
        <div class="wrap" id="wrap"><input type="email" id="mail" value="ana.maria.silva@"></div>
        <div class="layer" id="layer"></div>`;
      for (const pick of root.querySelectorAll('[data-flight]')) {
        pick.addEventListener('click', record(pick.dataset.flight));
      }
      root.getElementById('card').addEventListener('click', record('card'));
      root.getElementById('info').addEventListener('click', (event) => {
        event.stopPropagation();
        calls.push({ what: 'info', trusted: event.isTrusted });
      });
      root.getElementById('cab').addEventListener('click', record('cab'));
      const shy = root.getElementById('shy');
      shy.addEventListener('mousemove', () => { root.getElementById('layer').style.display = 'block'; });
      shy.addEventListener('click', record('shy'));
      root.getElementById('layer').addEventListener('click', record('layer'));
    }
  }
  customElements.define('x-results', XResults);
</script>
</body></html>"#;

/// The ref of the first `clickable` line named `name` after the line that
/// shows `marker` in `tree`.
#[cfg(feature = "agent-browser")]
fn ref_after(tree: &str, marker: &str, name: &str) -> String {
    let wanted = format!("- clickable \"{name}\" [ref=");
    let line = tree
        .lines()
        .skip_while(|line| !line.contains(marker))
        .find(|line| line.contains(&wanted))
        .unwrap_or_else(|| panic!("no {name:?} after {marker:?} in\n{tree}"));
    let at = line.find("[ref=").expect("a ref") + "[ref=".len();
    line[at..line[at..].find(']').expect("a ref's end") + at].to_owned()
}

/// What the page recorded as pressed, by name.
#[cfg(feature = "agent-browser")]
async fn pressed(
    browser: &crate::Browser,
    id: &tinycomputer_bus::browser::SessionId,
) -> Vec<String> {
    let calls = page_value(browser, id, "window.calls.map((call) => call.what)").await;
    serde_json::from_value(calls).expect("the calls are names")
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_each_select_goes_on_its_own_flight_whatever_comes_before_it() {
    let Some((browser, id)) = live_page(RESULTS).await else {
        return;
    };
    let full = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    eprintln!("{}", full.tree);
    // Flight A's "Select" is no control, and stays as it was.
    let flight_a = full
        .tree
        .lines()
        .skip_while(|line| !line.contains("\"Flight A\""))
        .nth(1)
        .unwrap();
    assert_eq!(flight_a.trim(), "- StaticText \"Select\"");
    let flight_b = ref_after(&full.tree, "\"Flight B\"", "Select");
    let flight_c = ref_after(&full.tree, "\"Flight C\"", "Select");
    assert_ne!(flight_b, flight_c);
    on(&browser, &id, &flight_b, click).await.unwrap();
    assert_eq!(pressed(&browser, &id).await, ["B"]);
    on(&browser, &id, &flight_c, click).await.unwrap();
    assert_eq!(pressed(&browser, &id).await, ["B", "C"]);
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_row_redrawn_in_place_to_show_another_item_is_stale() {
    let Some((browser, id)) = live_page(RESULTS).await else {
        return;
    };
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let mini = ref_of(&shot, "clickable", "Mini");
    // The page reuses the row's own node for another item, as a list that
    // recycles its rows does: the mark stays, the words change.
    page_value(
        &browser,
        &id,
        "document.querySelector('x-results').shadowRoot.getElementById('cab').textContent = 'Auto'; true",
    )
    .await;
    let stale = on(&browser, &id, &mini, click).await.unwrap_err();
    assert!(
        matches!(&stale, Error::StaleRef { reference } if *reference == mini),
        "{stale}"
    );
    let read = on(&browser, &id, &mini, |target| Action::GetText { target });
    assert!(matches!(read.await, Err(Error::StaleRef { .. })));
    assert_eq!(pressed(&browser, &id).await.len(), 0, "nothing was pressed");
    // A fresh snapshot names it anew.
    let again = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_ne!(ref_of(&again, "clickable", "Auto"), mini);
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_card_is_pressed_beside_its_own_button_never_on_it() {
    let Some((browser, id)) = live_page(RESULTS).await else {
        return;
    };
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let card = ref_of(&shot, "clickable", "Mumbai Info Delhi");
    on(&browser, &id, &card, click).await.unwrap();
    assert_eq!(pressed(&browser, &id).await, ["card"]);
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_press_something_comes_over_on_its_way_is_stopped() {
    let Some((browser, id)) = live_page(RESULTS).await else {
        return;
    };
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    // The pointer's arrival raises a layer over "Prime", as a hover card
    // can: the press would land on the layer.
    let prime = ref_of(&shot, "clickable", "Prime");
    let stopped = on(&browser, &id, &prime, click).await.unwrap_err();
    assert!(
        matches!(&stopped, Error::NotActionable { reason } if reason.contains("<div#layer> came over it")),
        "{stopped}"
    );
    assert_eq!(pressed(&browser, &id).await.len(), 0, "neither was pressed");
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_text_goes_after_what_a_field_with_no_caret_to_set_holds() {
    let Some((browser, id)) = live_page(RESULTS).await else {
        return;
    };
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    // The box around the field shows no words of its own.
    let wrap = ref_of(&shot, "clickable", "");
    let typed = on(&browser, &id, &wrap, |target| Action::Type {
        target: Some(target),
        text: "example.com".to_owned(),
        delay_ms: None,
    });
    typed.await.unwrap();
    assert_eq!(
        page_value(
            &browser,
            &id,
            "document.querySelector('x-results').shadowRoot.getElementById('mail').value"
        )
        .await,
        json!("ana.maria.silva@example.com")
    );
    browser.close_session(&id).await.unwrap();
}
