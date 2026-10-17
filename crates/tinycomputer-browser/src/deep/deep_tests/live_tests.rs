//! Live tests of deep controls in a real browser, gated on the
//! `agent-browser` feature and `TINYCOMPUTER_LIVE_BROWSER=1`, since CI has
//! no browser to launch: a page built from web components, whose controls
//! agent-browser leaves without refs, snapshotted and acted on through
//! [`crate::Browser`]. The browser launched is the one
//! `TINYCOMPUTER_BROWSER_EXECUTABLE` names, else the one the engine finds.

#[cfg(feature = "agent-browser")]
use serde_json::{Value, json};
#[cfg(feature = "agent-browser")]
use tinycomputer_bus::browser::{
    Action, SessionId, SessionOptions, Snapshot, SnapshotRequest, Target,
};

#[cfg(feature = "agent-browser")]
use crate::Browser;
#[cfg(feature = "agent-browser")]
use crate::error::Error;

/// A page built from web components: a pointer `span` to log in one shadow
/// root down, rows two down (one with no words, a chevron), an editable
/// region, a native button and field (refs of agent-browser's own), words
/// after it all, and a closed shadow root that cannot be read. Every press
/// and hover is recorded in `window.calls`, with whether it was trusted.
#[cfg(feature = "agent-browser")]
const COMPONENTS: &str = r#"<!doctype html>
<html><head><style>body { margin: 0; font: 16px sans-serif; }</style></head>
<body>
<x-app></x-app>
<p>Please log in</p>
<x-closed></x-closed>
<script>
  window.calls = [];
  const record = (what) => (event) => calls.push({ what, trusted: event.isTrusted });
  class XApp extends HTMLElement {
    constructor() {
      super();
      const root = this.attachShadow({ mode: 'open' });
      root.innerHTML = `<style>.ptr { cursor: pointer; } .login { text-transform: uppercase; }</style>
        <header><span class="ptr login" id="login">Log in</span></header>
        <x-list></x-list>
        <button id="native">Native</button>
        <input id="field" placeholder="Where to">
        <div class="editor" contenteditable="true" style="min-height: 24px; border: 1px solid #999"></div>`;
      const login = root.getElementById('login');
      login.addEventListener('click', record('login'));
      login.addEventListener('mouseenter', record('hover login'));
    }
  }
  class XList extends HTMLElement {
    constructor() {
      super();
      this.attachShadow({ mode: 'open' });
      this.render();
    }
    render() {
      this.shadowRoot.innerHTML = `<style>.ptr { cursor: pointer; } .row { padding: 8px; }</style>
        <div class="row ptr" data-cab="mini"><span>Mini</span> <span>3 min</span></div>
        <div class="row ptr" data-cab="chevron"><svg width="12" height="12"><path d="M2 2 L10 6 L2 10"/></svg></div>`;
      for (const row of this.shadowRoot.querySelectorAll('.row')) {
        row.addEventListener('click', record(row.dataset.cab));
      }
    }
  }
  class XClosed extends HTMLElement {
    constructor() {
      super();
      const root = this.attachShadow({ mode: 'closed' });
      root.innerHTML = '<span style="cursor: pointer">Hidden away</span>';
    }
  }
  customElements.define('x-app', XApp);
  customElements.define('x-list', XList);
  customElements.define('x-closed', XClosed);
</script>
</body></html>"#;

/// A page with no shadow root: controls, a pointer row agent-browser refs
/// itself, and words.
#[cfg(feature = "agent-browser")]
const LIGHT: &str = r#"<!doctype html>
<html><body>
<h1>Rides</h1>
<button>Search</button>
<a href="/help">Help</a>
<div style="cursor: pointer" onclick="void 0">Mini 3 min</div>
<p>Please log in</p>
</body></html>"#;

/// A real browser with `html` written into a blank tab: `None` unless
/// `TINYCOMPUTER_LIVE_BROWSER=1`.
#[cfg(feature = "agent-browser")]
pub(super) async fn live_page(html: &str) -> Option<(Browser, SessionId)> {
    use std::sync::Arc;

    if std::env::var("TINYCOMPUTER_LIVE_BROWSER").as_deref() != Ok("1") {
        return None;
    }
    let browser = Browser::new(Arc::new(crate::AgentBrowser));
    let options = SessionOptions {
        executable: std::env::var("TINYCOMPUTER_BROWSER_EXECUTABLE").ok(),
        ..SessionOptions::default()
    };
    let info = browser
        .open_session(options)
        .await
        .expect("a browser launches when live runs are asked for");
    let write = format!(
        "document.open(); document.write({}); document.close(); true",
        Value::String(html.to_owned())
    );
    browser
        .command(&info.id, json!({"action": "evaluate", "script": write}))
        .await
        .expect("the fixture is written");
    Some((browser, info.id))
}

/// A snapshot as an agent's host reads a page: compact, not only the
/// interactive lines, cut at 24,000 characters.
#[cfg(feature = "agent-browser")]
pub(super) fn compact() -> SnapshotRequest {
    SnapshotRequest {
        compact: true,
        max_chars: 24_000,
        ..SnapshotRequest::default()
    }
}

/// The ref `snapshot` lists for the element named `name` with `role`.
#[cfg(feature = "agent-browser")]
pub(super) fn ref_of(snapshot: &Snapshot, role: &str, name: &str) -> String {
    snapshot
        .refs
        .iter()
        .find(|element| element.role == role && element.name == name)
        .unwrap_or_else(|| panic!("no {role} {name:?} in {:#?}", snapshot.refs))
        .id
        .clone()
}

/// What the page's script evaluates `expression` to.
#[cfg(feature = "agent-browser")]
pub(super) async fn page_value(browser: &Browser, id: &SessionId, expression: &str) -> Value {
    browser
        .command(id, json!({"action": "evaluate", "script": expression}))
        .await
        .expect("the expression runs")["result"]
        .clone()
}

/// `action` on `target`, as `Browser::perform` does it.
#[cfg(feature = "agent-browser")]
pub(super) async fn on(
    browser: &Browser,
    id: &SessionId,
    target: &str,
    action: fn(Target) -> Action,
) -> crate::error::Result<Value> {
    browser
        .perform(id, action(Target::reference(target)))
        .await
        .map(|outcome| outcome.value)
}

#[cfg(feature = "agent-browser")]
pub(super) fn click(target: Target) -> Action {
    Action::Click {
        target,
        new_tab: false,
    }
}

#[cfg(feature = "agent-browser")]
const EDITOR_TEXT: &str =
    "document.querySelector('x-app').shadowRoot.querySelector('.editor').innerText";

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_controls_inside_shadow_roots_get_refs_of_their_own() {
    let Some((browser, id)) = live_page(COMPONENTS).await else {
        return;
    };
    let shot = browser.snapshot(&id, compact()).await.unwrap();
    eprintln!("{}", shot.tree);
    let login = ref_of(&shot, "clickable", "LOG IN");
    let mini = ref_of(&shot, "clickable", "Mini 3 min");
    let chevron = ref_of(&shot, "clickable", "");
    let editor = ref_of(&shot, "editable", "");
    for deep in [&login, &mini, &chevron, &editor] {
        assert!(deep.starts_with('d'), "{deep}");
        assert!(
            shot.tree.contains(&format!("[ref={deep}]")),
            "{deep} is in the tree"
        );
    }
    assert!(
        shot.tree
            .contains(&format!("- clickable \"LOG IN\" [ref={login}]"))
    );
    // agent-browser's own refs stand, and are not listed twice; the closed
    // root's pointer cannot be reached, and costs nothing.
    assert!(ref_of(&shot, "button", "Native").starts_with('e'));
    assert!(ref_of(&shot, "textbox", "Where to").starts_with('e'));
    assert!(!shot.refs.iter().any(|element| element.id.starts_with('d')
        && (element.name == "Native" || element.name == "Hidden away")));
    // Read in full, the tree shows their words where the page draws them,
    // and that is where they go; read again, they keep their numbers.
    let full = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    eprintln!("{}", full.tree);
    let group = full
        .tree
        .find("- group \"Controls inside web components\"")
        .map_or("", |at| &full.tree[at..]);
    assert!(
        !group.contains("LOG IN") && !group.contains("Mini 3 min"),
        "{}",
        full.tree
    );
    assert_eq!(ref_of(&full, "clickable", "LOG IN"), login);
    assert_eq!(ref_of(&full, "clickable", "Mini 3 min"), mini);
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_deep_press_is_a_persons_and_a_redrawn_row_is_stale() {
    let Some((browser, id)) = live_page(COMPONENTS).await else {
        return;
    };
    let shot = browser.snapshot(&id, compact()).await.unwrap();
    let login = ref_of(&shot, "clickable", "LOG IN");
    let mini = ref_of(&shot, "clickable", "Mini 3 min");
    assert_eq!(on(&browser, &id, &mini, click).await.unwrap(), Value::Null);
    assert_eq!(
        page_value(&browser, &id, "window.calls").await,
        json!([{"what": "mini", "trusted": true}])
    );
    on(&browser, &id, &login, |target| Action::Hover { target })
        .await
        .unwrap();
    assert_eq!(
        page_value(&browser, &id, "window.calls.at(-1)").await,
        json!({"what": "hover login", "trusted": true})
    );
    let text = on(&browser, &id, &login, |target| Action::GetText { target });
    assert_eq!(text.await.unwrap(), json!("LOG IN"));
    let shows = on(&browser, &id, &login, |target| Action::IsVisible { target });
    assert_eq!(shows.await.unwrap(), json!(true));
    // What lies over the element is named, and nothing is pressed.
    page_value(
        &browser,
        &id,
        "const mask = document.createElement('div'); mask.className = 'mask dim'; \
         mask.style.cssText = 'position: fixed; inset: 0'; document.body.append(mask); true",
    )
    .await;
    let covered = on(&browser, &id, &login, click).await.unwrap_err();
    assert!(
        matches!(&covered, Error::NotActionable { reason } if reason.contains("is covered by <div.mask.dim>")),
        "{covered}"
    );
    page_value(
        &browser,
        &id,
        "document.querySelector('.mask').remove(); true",
    )
    .await;
    // Re-drawn, the row is a new element: its old ref is stale, and presses
    // nothing.
    page_value(
        &browser,
        &id,
        "document.querySelector('x-app').shadowRoot.querySelector('x-list').render(); true",
    )
    .await;
    let stale = on(&browser, &id, &mini, click).await.unwrap_err();
    assert!(
        matches!(&stale, Error::StaleRef { reference } if *reference == mini),
        "{stale}"
    );
    assert_eq!(
        page_value(
            &browser,
            &id,
            "window.calls.filter((call) => call.what !== 'hover login')"
        )
        .await,
        json!([{"what": "mini", "trusted": true}])
    );
    // A fresh snapshot keeps the number of what still lives, and names the
    // re-drawn row anew.
    let again = browser.snapshot(&id, compact()).await.unwrap();
    assert_eq!(ref_of(&again, "clickable", "LOG IN"), login);
    assert_ne!(ref_of(&again, "clickable", "Mini 3 min"), mini);
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_text_goes_into_the_editable_region_and_nowhere_else() {
    let Some((browser, id)) = live_page(COMPONENTS).await else {
        return;
    };
    let shot = browser.snapshot(&id, compact()).await.unwrap();
    let login = ref_of(&shot, "clickable", "LOG IN");
    let editor = ref_of(&shot, "editable", "");
    let typed = on(&browser, &id, &editor, |target| Action::Type {
        target: Some(target),
        text: "Kora".to_owned(),
        delay_ms: None,
    });
    typed.await.unwrap();
    assert_eq!(page_value(&browser, &id, EDITOR_TEXT).await, json!("Kora"));
    let filled = on(&browser, &id, &editor, |target| Action::Fill {
        target,
        value: "Airport".to_owned(),
    });
    filled.await.unwrap();
    assert_eq!(
        page_value(&browser, &id, EDITOR_TEXT).await,
        json!("Airport")
    );
    // Text never goes to what takes none, which is not even pressed, and
    // the focus is never left where it was: the log-in span takes none.
    let typed = on(&browser, &id, &login, |target| Action::Type {
        target: Some(target),
        text: "x".to_owned(),
        delay_ms: None,
    });
    assert!(matches!(
        typed.await,
        Err(Error::NotActionable { reason }) if reason.contains("takes no typed text")
    ));
    let focused = on(&browser, &id, &login, |target| Action::Focus { target });
    assert!(matches!(
        focused.await,
        Err(Error::FocusMissed { focused }) if focused == "nothing"
    ));
    assert_eq!(page_value(&browser, &id, "window.calls").await, json!([]));
    assert_eq!(
        page_value(&browser, &id, EDITOR_TEXT).await,
        json!("Airport")
    );
    browser.close_session(&id).await.unwrap();
}

#[cfg(feature = "agent-browser")]
#[tokio::test]
async fn live_a_page_without_shadow_roots_is_snapshotted_as_the_engine_gives_it() {
    let Some((browser, id)) = live_page(LIGHT).await else {
        return;
    };
    for request in [SnapshotRequest::default(), compact()] {
        let shot = browser.snapshot(&id, request.clone()).await.unwrap();
        let engine = browser
            .command(
                &id,
                json!({"action": "snapshot", "compact": request.compact, "interactive": false, "urls": false}),
            )
            .await
            .unwrap();
        assert_eq!(shot.tree, engine["snapshot"].as_str().unwrap());
        assert_eq!(shot.refs.len(), engine["refs"].as_object().unwrap().len());
        assert!(!shot.refs.iter().any(|element| element.id.starts_with('d')));
    }
    assert_eq!(
        page_value(
            &browser,
            &id,
            "document.querySelectorAll('[data-tc-deep]').length"
        )
        .await,
        json!(0)
    );
    browser.close_session(&id).await.unwrap();
}
