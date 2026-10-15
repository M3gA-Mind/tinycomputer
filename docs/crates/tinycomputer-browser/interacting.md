# Clicking, typing, keys, and going back

Code: `crates/tinycomputer-browser/src/convert/` (contract to engine
commands), `crates/tinycomputer-browser/src/surface/operations.rs` (the `Surface`
implementation a decision loop actually calls). Action definitions:
`crates/tinycomputer-bus/src/browser/action/types.rs`.

## Two ways to drive a session

You can drive a `Browser` session two ways, and both end up going through
the same `convert` functions and the same agent-browser engine:

- **Directly**, by calling `Browser::perform(session_id, Action)` with one
  of the `Action` variants (`Click`, `Fill`, `Type`, `Press`, `Scroll`,
  `Check`, ...). This is what a caller talking straight to the crate uses.
- **Through `BrowserSurface`**, which `tinycomputer-engine`'s decision loops
  drive with a small, surface-agnostic vocabulary
  (`JevOperation::Click`, `TypeText`, `Check`, `Scroll`, ...) that
  `BrowserSurface::execute` translates into the same `Action`s underneath.
  See [surface.md](surface.md) for that translation.

This page is mostly about the second path, because it is where the
interesting behavior lives. The direct path is a straightforward
`Action → JSON command` mapping in `convert/interaction.rs`, with no logic of its own
beyond the focus check every action that sends keys passes first (see "Keys go only
where an action points").

## Targets: ref, selector, or locator

Every action that touches an element names a `Target`:

- `Target::Ref`: a ref from a snapshot (`e12`, or `d3` for a control inside
  a web component, see below) or a sight mark (`seen:12`). Refs belong to
  the reading that produced them; acting on one after the page has
  re-rendered is refused rather than guessed at.
- `Target::Selector`: a raw CSS selector, matched against the first
  element found.
- `Target::Locator`: a semantic locator (role, text, label, placeholder,
  test id, alt text, or title), which agent-browser resolves itself through
  its `getbyrole`/`getbytext`/... family of commands. Locators only work
  with a handful of actions (click, fill, check, hover, get text); anywhere
  else, `convert::selector` refuses with `Error::InvalidInput` naming the
  ones that do.

`BrowserSurface` never builds a `Target::Selector` from a plain ref by hand.
`target()` in `surface/operations.rs` decides, per reference, whether it is a sight
mark (routed through `sight::selector`, which turns `seen:12` into
`[data-tc-seen="12"]`) or a tree ref (routed through `Target::reference`,
which becomes agent-browser's own `@e12` form).

## Controls inside web components: deep refs

Code: `crates/tinycomputer-browser/src/deep/` (the look, and placing what it
finds in the tree), `src/sessions/deep.rs` (acting on one).

A page built from web components keeps its controls inside shadow roots.
agent-browser refs what has a control's role there as anywhere, a native
button or field included, but a component's own control is often a plain
`span` or `div` drawn with a pointer and wired to a click by its script: no
role, no tab stop. agent-browser looks for such elements by their pointer in
the page's light DOM only, so inside a component they come out as bare text,
or, in a compact snapshot, not at all. Live, a booking page built this way
showed its log-in link as plain text and dropped its list of results.

So `Browser::snapshot` follows agent-browser's snapshot with one look
(`deep.js`) through the page's open shadow roots for what a person can press
or type into that the tree has no ref for: a pointer of its own (not one
inherited from a row around it), an `onclick`, a tab stop, an editable
region. Only what is drawn and not hidden from people counts, only the
outermost of nested ones (a field inside a pressable row stays a field), and
at most 40, in the order the page draws them. Each is marked
`data-tc-deep="N"` and listed as ref `dN`, with the role `clickable`,
`editable`, or `focusable`:

- on the innermost line whose whole text is its words, which becomes it:
  `- StaticText "LOG IN"` becomes `- clickable "LOG IN" [ref=d1]`;
- otherwise after the tree, under `- group "Controls inside web components"`,
  which a cut to `max_chars` never drops: the tree before it is cut shorter.

`Snapshot::refs` lists them after agent-browser's. A page with nothing to
list comes out exactly as agent-browser gave it. A closed shadow root cannot
be read and is passed over, as are frames. A number is never handed out
twice in a session, and an element keeps its number from one snapshot to
the next while it lives.

`Browser::perform` acts on a `d` ref itself, never through agent-browser,
which cannot resolve it. The element is found by its mark (gone, or carried
by two elements: `StaleRef`), brought into the window, and the middle of
what the window shows of it checked, through open shadow roots, to be the
element or inside it (otherwise `NotActionable`, naming what covers it by
tag, id, and class). Then:

- `Click` presses there with raw mouse events, moved, pressed, released
  (`convert::press_at`, the press the surface uses on a point), so the page
  sees a person's trusted press with the button going down, never a
  script's `click()`; `Hover` moves the pointer there;
- `Type` and `Fill`, only on an element that takes text or holds something
  that does, press it as a person clicks into a field, and go ahead only if
  the focus landed in it (`FocusMissed` otherwise, as below): `Type` types
  a character at a time after what it holds, `Fill` replaces all it holds;
- `Focus`, and `Press` with a key, give it the focus without a press,
  checked the same way;
- `GetText` and `IsVisible` read it.

Any other action on a `d` ref is `InvalidInput`. A host that reads refs from
a model's text must take `d` refs as well as `e` refs.

## Clicking

`Action::Click` scrolls the element into view and clicks it. If another
element covers the click point (a consent banner, a modal, a sticky header),
agent-browser refuses the click and names the element in the way, rather
than clicking through it and reporting success on a click that landed
nowhere useful. That refusal message is what `reply::classify` turns into
`Error::NotActionable`.

A selector's element that shows at all is not scrolled, though, and its
press lands on its middle whether the window shows that point or not, with
success reported. So before pressing a control sight found, the surface
brings it to the middle of the window when its middle lies outside the
window (`INTO_VIEW_JS` in `surface/uncover.rs`), and leaves a control in
view where it is. Live, a store's "Add to cart" sat at the window's foot
with its middle below it, and was pressed again and again with nothing
happening. A ref of the tree is brought into view by agent-browser itself.

### Click-through on result cards

A refusal because something is "covered" is not always the end of the
story. Plenty of result lists, flight search results, hotel cards, search
engine results, lay a transparent click layer, or the card's own visible
text, directly over the exact link or button you meant to click. From a
pixel's point of view, the card's own content is "covering" its own
control.

`BrowserSurface::click_through_own_card` (in `surface/card.rs`) handles this
case specifically, and only this case. When a click comes back covered, and
the target has either a name or a sight mark, it:

1. Reads the target's bounding box.
2. Runs a small script (`SAME_CARD_JS`) that finds, among the elements
   stacked at that point, the one whose visible text or `aria-label`
   matches the target's name *exactly* (whitespace runs squashed to one
   space), or, for a sight-minted ref, the element carrying that exact
   mark.
3. Checks that whatever is actually on top at that point sits inside the
   *same* card (`li`, `[role="listitem"]`, `[role="row"]`, `article`,
   `[role="article"]`) as the target, and inside no dialog.
4. Only if both hold, dispatches the click as raw mouse events
   (`mouseMoved`, `mousePressed`, `mouseReleased`) at that point instead of
   asking agent-browser to click the element directly.

The exact-match requirement on step 2 exists because a short name like
"Select" turns up on nearly every card in a results page. Containment
alone is not enough to tell which card's "Select" you meant, so the code
insists on an exact match against the element's own shown text. Google
Flights is the real example that shaped this: each card's duration text
sits visually above its "Select flight" link, and Google renders each
flight card twice, once inside a hidden tab, so containment checks that
ignore layer boundaries would misfire.

A banner or dialog genuinely in front of the whole card still blocks the
click. Step 3's dialog check exists specifically so this shortcut never
becomes a way to click through something that is actually blocking the
page, only through a card's own content sitting on top of itself.

### Once more, centred, and then naming what is in the way

A press still covered after that, on a control sight found, is tried once
more by `click_uncovered` (`surface/uncover.rs`): the focused text box is
let go, so a list of suggestions it holds open can close, the control is
brought to the middle of the window at once (`CENTRE_JS`, with
`behavior: 'instant'`: on a page that sets `scroll-behavior: smooth`, a
smooth scroll would still be moving the control when the press lands), the
pointer moves off to the window's corner, so a panel it raised by hovering
closes, and the press goes again.

A press refused even then says what covers its target, in the error's
`details`, read by `COVER_JS` (`surface/cover.rs`) at the target's middle,
looking into open shadow roots, where a page's components draw their
controls:

- `cover`: what is on top there, as a person would say it: a control by its
  role and name (`button "Select Location"`), a dialog, a layer by the text
  it shows or holds, a picture, or `an empty layer`;
- `empty_layer`: whether that is an empty layer a press outside a popup
  closes, as the backdrop a popup or a box's list of suggestions leaves over
  a page is. It must cover at least half the window, with no control,
  dialog, text, or picture where it covers the target; belong to no
  component (no shadow root, no custom element); be placed over the page
  (it, or its nearest placed ancestor, is `fixed` or `absolute`); and lie
  over a control sight found by its mark, so the target can be told apart
  from what lies beneath. A ref of the tree never has one.

The flow's history then names the cover instead of "something", and a step
that fails on it says so. Nothing is named when nothing shows at the
target's middle.

`Surface::dismiss_cover` (`BrowserSurface::press_cover`) presses such an
empty layer the way a person clicks outside a popup to close it. Of 49
points spread evenly over the window (`SPOT_GRID`), it takes the first
where the layer itself is on top and, beneath it, nothing could take a
press: no control, nothing that shows a pointer, nothing of the target.
The layer stops taking the pointer only for the instant each point beneath
it is read, and its own style is put back as it was. The press goes there
as raw mouse events, with the on-screen cursor gliding to the point in a
visible session, so even a layer that closes on the button going down lets
nothing beneath take it; then the layer is given `LAYER_GONE_MS` (150 ms)
to go. A cover that is a control, a dialog, or anything that shows
something is never pressed for its target, and neither is an empty layer
with no such point, or one that cannot be read: the call is refused with
`NOT_ACTIONABLE` (the cover in `details.cover` when it was read).
`dismissed` is `null` when nothing covers the target any more. Live, a
store's search box left its suggestions open over the page, every control
outside them read as covered, and Escape left them there: its basket button
was refused until rescues added the wrong items.

### Selecting tabs, radios, and options that ignore a trusted click

Some pages simply do not react to a synthetic click the moment they have
just rendered. Emirates' own trip-type tabs, freshly loaded, are the
observed case, while the exact same element's own `element.click()` works
fine once the page's own handlers are attached. After a successful click on
something the flow expects to *select* (a `tab`, `radio`, or `option` that
is not already marked selected or checked,
`surface::mod::selects_on_click`), the surface checks whether the click
actually left the element selected. If not, and the ref is a sight mark, it
presses the element again through the DOM (`SELECT_JS`) rather than giving
up. Selecting is idempotent, so pressing an already-selected element a
second time is harmless; a checkbox, which *toggles* rather than selects,
is never pressed this way.

## Keys go only where an action points

agent-browser types and presses into whatever holds the keyboard focus. It
asks the target to take the focus first, but an element that takes none (a
heading, a label) leaves the focus where it was, so text typed "into" a
heading lands in the field filled before it, a card number say, and a key
pressed on one presses whatever button last had the focus.

So `Browser::perform` checks first (`sessions/focus.rs`), for `Focus`,
`Fill`, `Type` with a target, and `Press` with a target:

1. It takes the focus off whatever holds it, reaching through open shadow
   roots and same-origin frames.
2. It gives the target the focus (agent-browser's `focus`).
3. It marks the elements now holding the focus with a fresh random value
   (`data-tc-focus`) and asks agent-browser for that attribute on the
   target, which resolves the target in its own frame. Only the target, or
   a shadow host around it, carries the mark. The marks come off again
   either way.
4. If the mark is not on the target, the action fails with
   `Error::FocusMissed` (`NOT_ACTIONABLE` on the wire, naming only the tag
   that holds the focus) and nothing is typed or pressed.

Two limits. Inside a cross-origin frame (a payment provider's card field)
the page cannot see where the focus is; with the focus taken off everything
first, it can only be there because the target took it, so that goes ahead.
And a page's own scripts run between these steps and could fool the check:
it stops mistakes, not a page set on defeating it. A locator's `Fill`, which
only agent-browser resolves, cannot be checked; the focus is taken off
whatever held it first, so its text lands in the field or nowhere. `Type`
and `Press` without a target still go wherever the focus is.

## Typing text

There are two distinct actions, on purpose:

- `Action::Fill` clears a field and sets its value in one step. Fast, but a
  field that reacts to individual keystrokes (an autocomplete, a
  search-as-you-type box) never sees the input events a bulk assignment
  skips.
- `Action::Type` sends text as a real sequence of key events, either into a
  named target or, with no target, wherever the browser's focus currently
  is (which becomes agent-browser's `inserttext` command).

`BrowserSurface::execute` never lets `TypeText` land somewhere by accident.
With a target, it checks `takes_text` first, actually focusing the element
and then checking what ended up focused rather than what role the element
claims, and refuses with `NOT_A_TEXT_FIELD` rather than typing into, say, a
`div` that merely claims `role="textbox"`. Without a target (typing into
"whatever has focus", as into an autocomplete's own newly-opened, unnamed
input), it checks `focused_field_is_editable` first for the same reason: a
stale or unexpected focus must never silently receive typed text, including
a private value pulled from a fact.

`focused_field_is_editable`'s check descends into open shadow roots and
same-origin iframes before judging editability, so that focus checks still
work for the documented shadow-root/frame fallback case, not just the plain
top-level page.

## Pasting without a clipboard

`BrowserSurface::paste` types at a field's caret without touching an
actual system clipboard: focus the target, select everything already there
with `Ctrl+A`/`Cmd+A` when the target supports `SetValue`, then insert the
text. It refuses up front if the target does not turn out to be editable,
the same `NOT_A_TEXT_FIELD` case as typing.

## Pressing keys

`Action::Press` sends one key or chord (`Enter`, `Tab`, `Control+a`), to
whatever has the focus, or, with a `target`, to that element once the focus
is checked to be on it (see "Keys go only where an action points").
`BrowserSurface::press` takes a logical combo string like `cmd+a` or
`return` and spells it the way agent-browser expects (`browser_key` in
`surface/operations.rs`): the logical `cmd` becomes whichever platform key
`tinycomputer-core`'s keymap says is "select all" on this OS, arrow names
become `ArrowUp`/`ArrowDown`/etc., and anything else is capitalized to
match agent-browser's naming (`return`/`enter` → `Enter`,
`escape`/`esc` → `Escape`).

## Scrolling and waiting

`Action::Scroll` scrolls the page or a scrollable element within it, in
CSS pixels, defaulting to 300px per call; `ScrollDirection::Top`/`Bottom`
scroll a very large distance (`TO_THE_END`, one million pixels) to reach
either end regardless of document length. `Action::WaitFor` blocks on text
appearing anywhere on the page, an element reaching a given state
(`attached`, `detached`, `visible`, `hidden`), or a flat delay; `convert::
wait_for` refuses if none of the three is given.

`Surface::settle`, called before a decision loop reads the page again,
waits (bounded, up to `QUIET_MS` = 1 second) for the requests that change
the page to end, then only while the page is still changing. A page that
polls constantly in the background never goes properly quiet, so the
network wait is a cap, not a guarantee.

That is `Settle::Prompt`, the default (the module's `browser.settle`). It
waits for the engine's `networkquiet`, which counts its 500 ms of quiet
from the start and counts only requests that can change what the page
shows: the page's own document, scripts, stylesheets, and fetched data, not
analytics pings, pictures, fonts, media, or other frames' documents (some of
which never report finishing at all). Those the action itself sent before
the wait began, up to 3 s earlier, are waited for too. Live on Amazon, every
action that opened a page sent 100+ requests for over 2 s, while what the
task needed showed after 0.7–1.2 s. It then resolves once no DOM change has happened for
`STILL_MS` = 120 milliseconds and no finite CSS animation or transition is
running (a menu fading out changes no DOM node), over at least two drawn
frames, and after `SETTLE_MS` = 400 milliseconds at most. An endless spinner
is not waited for, and a busy page still waits for its requests: an idle
page is read again after about 0.6 s. The page and its open shadow roots are
watched alike, and each call has a deadline 500 ms past its own cap, so one
sent while a page is replaced cannot wait out the browser's 30 s.

`Settle::Steady`, the earlier default, waits for `networkidle` instead,
which starts counting its 500 ms of quiet only after a first 600 ms
receive window, and then pauses `SETTLE_MS` regardless, giving a banner or
menu that is mid-animation time to finish closing: about 1.6 s an action,
live.

`Surface::settle_briefly` follows an action that fetches nothing. Under
`Settle::Prompt` it skips the network wait and waits only while the page
changes (120–400 ms instead of about 0.65 s on an idle page); under
`Settle::Steady` it settles in full. A flow settles briefly only after a
launch, which leaves an open page as it is, and after Escape closing a layer:
live, 3% of launches and 12% of Escapes settled with a request of the page
still running, against 39% of fills, whose suggestions the next look reads,
and 70% of clicks.

`Surface::await_change(ms)` watches the page rather than pausing: one
`evaluate` whose `MutationObserver` resolves `true` at the page's first change
of its own (an element or words added, removed, or rewritten, or an
element's look changed, never a `data-tc-` mark sight leaves), or `false`
once `ms` pass with none. A flow waiting for a place box's late suggestions
settles and looks again as soon as the page changes, and stops waiting once
it stays still; a watch that cannot run, or with no page open, says the page
may have changed.

## Going back

`Surface::back` maps straight to `Action::Back`, which agent-browser
resolves to the browser's own history-back. There is no separate "forward"
exposed through the `Surface` trait (`Action::Forward` exists on the direct
`Browser::perform` path, just not wired into `BrowserSurface::execute`).

## Reading a value back

`Surface::read_value` reads what a field currently holds, trying
`inputvalue` first and falling back to `gettext`, useful for verifying what
actually landed in a field after a fill, especially one that went through
an autocomplete that rewrote it.
