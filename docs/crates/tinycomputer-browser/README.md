# tinycomputer-browser

tinycomputer-browser is the browser half of tinycomputer, a decision model
(Jev) based harness for desktop and browser automation written in Rust. It
adapts [agent-browser] into that harness: typed requests like "open a
session", "click this", "read the page" become commands for agent-browser,
and agent-browser's replies become typed results the rest of tinycomputer can
use. It is the browser counterpart of `tinycomputer-desktop`: same job,
different target.

[agent-browser]: https://github.com/vercel-labs/agent-browser

## Who needs this page

You are in the right place if you want to know how tinycomputer actually
drives a web page: how a session starts, how it decides what is on the page
and what you can click, what happens when a click is covered by something
else, and what a caller gets back when something goes wrong. If you want the
exhaustive wire contract instead, read
[`../../technical/specs/browser-sight.md`](../../technical/specs/browser-sight.md)
and [`../../technical/specs/unified-agent.md`](../../technical/specs/unified-agent.md),
or the crate's own rustdoc.

If you are looking for how the whole system decides what to do next, not how
the browser adapter works, start at [`../../how-it-works.md`](../../how-it-works.md)
and [`../../how-it-decides.md`](../../how-it-decides.md) instead.

## What it is not

This crate has no bus, no agent loop, and no model. It does not decide what
to click; that is `tinycomputer-engine`'s job, driving this crate through the
`Surface` trait it implements. It does not run a daemon or a sidecar either:
agent-browser is linked in-process as a library, so there is one process,
not two talking over a socket. Putting `Browser` on the bus at all is
`crates/tinycomputer`'s job: its `DesktopService` holds one `Browser`, shared
with the task runner, and answers the 13 `Browser…` bus members by calling
straight into it. See
[docs/crates/tinycomputer-bus/browser.md](../tinycomputer-bus/browser.md) for
the wire shapes those members carry.

## The pieces, in one pass

| Folder in the crate | What it holds | Read this page for detail |
|---|---|---|
| `src/sessions/` | `Browser`: up to eight open sessions, each with its own engine and scratch directory | [sessions.md](sessions.md) |
| `src/engine/` | The seam to agent-browser: one JSON command in, one reply out. Everything above it is testable without a real browser | [sessions.md](sessions.md) |
| `src/linked/` | `AgentBrowser`, the real engine, wired to agent-browser's dispatcher | [sessions.md](sessions.md) |
| `src/convert/` | Turns contract requests (`Action`, `NavigateRequest`, ...) into the JSON commands agent-browser understands | [interacting.md](interacting.md) |
| `src/reply/` | Turns agent-browser's replies back into typed results, and classifies its failure messages into this crate's `Error` | [errors.md](errors.md) |
| `src/deep/` | Controls inside web components that the accessibility tree gives no ref, listed in every snapshot as `d` refs and acted on by position | [interacting.md](interacting.md#controls-inside-web-components-deep-refs) |
| `src/surface/sight/` | `sight.js`: reads the rendered page the way a person looks at it | [sight.md](sight.md) |
| `src/surface/tree.rs` | Parses agent-browser's accessibility snapshot text, as a fallback for sight | [sight.md](sight.md) |
| `src/surface/mod.rs`, `operations.rs`, `card.rs`, `fields.rs`, `envelope.rs`, `cursor.rs` | `BrowserSurface`: one session as a `tinycomputer-core` `Surface` (the trait impl in `operations.rs`), plus the on-screen cursor | [surface.md](surface.md) |
| `src/outputs/` | Held screenshots: bounded count, bounded size, expire on their own | [outputs-and-downloads.md](outputs-and-downloads.md) |
| `src/error/` | The crate-wide `Error`, and the wire name each variant carries across the bus | [errors.md](errors.md) |

## A tour, start to finish

1. A caller opens a session (`Browser::open_session`), or, over the bus,
   calls `BrowserOpenSession`. That either launches a fresh Chrome or
   attaches to one already running, depending on the options given. See
   [sessions.md](sessions.md).
2. The caller drives the session directly (navigate, click, read, screenshot)
   through `Browser`'s methods, or the `BrowserNavigate` / `BrowserPerform` /
   `BrowserReadPage` / `BrowserScreenshot` bus members that wrap them, or
   hands the session to `tinycomputer-engine` as a `BrowserSurface`, which
   drives it the same way a decision loop drives a desktop application. See
   [surface.md](surface.md).

   ```json
   // BrowserPerform request, over the bus, clicking a snapshot ref
   {"session": "s-1", "action": "click", "target": {"kind": "ref", "value": "e3"}}
   ```
3. Every "what's on the page" question is answered by *sight*: a script run
   in the page that reads it the way a person would, rather than trusting
   whatever roles and labels the page's own markup claims. When sight can't
   reach something, the crate falls back to agent-browser's accessibility
   tree. See [sight.md](sight.md).
4. Clicks, typing, key presses, and navigation go through a small,
   deliberately narrow set of actions, each translated into one agent-browser
   command. See [interacting.md](interacting.md).
5. Screenshots and downloads are held in memory or on disk until a caller
   collects them, then expire. See [outputs-and-downloads.md](outputs-and-downloads.md).
6. Anything that goes wrong comes back as one of a fixed set of errors, each
   telling the caller what to do next rather than just that something broke.
   See [errors.md](errors.md).

## Cross-links

- [`../../how-it-works.md`](../../how-it-works.md): how the whole system
  fits together, browser and desktop both.
- [`../../seeing-the-screen.md`](../../seeing-the-screen.md): observation in
  general, sight's desktop counterpart included.
- [`../../safety-and-privacy.md`](../../safety-and-privacy.md): why screen
  text and typed values are treated as data, never instructions, and never
  leaked.
- [`../../watching-a-run.md`](../../watching-a-run.md): what you can see
  while a browser session is being driven.
- [`../../glossary.md`](../../glossary.md): terms like ref, session,
  surface, and checkpoint.
- [`../../technical/specs/browser-sight.md`](../../technical/specs/browser-sight.md):
  the full sight specification, denoising rules, and acceptance criteria.
- [`../../technical/specs/unified-agent.md`](../../technical/specs/unified-agent.md):
  how the browser and desktop become one task API.
- [`../../technical/docker-lab.md`](../../technical/docker-lab.md): how to
  run this crate's live tests, which need a real Chromium.
