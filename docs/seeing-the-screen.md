# How it sees the screen

tinycomputer doesn't look at pixels. It reads the screen as a list of things
you can interact with: buttons, fields, links, checkboxes, list items, each
with a name and a state. This page explains where that list comes from on the
desktop and on the web, and how both end up looking the same to the rest of
the system.

## On the desktop: the accessibility tree

Every desktop operating system has an accessibility interface. Screen readers
use it to tell a blind user "button, New Message" or "text field, To, empty".
tinycomputer uses the same interface, through the vendored `agent-desktop`
engine.

A read of Mail's compose window comes back like this, in simplified form:

```text
button "Send"
textfield "To:" [focused]
textfield "Subject:"
textarea "Message body"
```

Each element also gets a **ref**, a short id like `@s8f3k2p9:e1`. A ref is
tied to the exact read it came from. When tinycomputer clicks `@s8f3k2p9:e1`,
it either reaches the element that was described or fails with `STALE_REF`,
meaning "the screen changed, read it again". It will never click whatever has
since moved into that spot.

Clicks go through the accessibility interface too. By default they don't move
your mouse, steal focus, or touch your clipboard, so a run can happen while
you use the machine. A "headed" mode sends real mouse and keyboard input for
the rare app that needs it.

### Permissions

The operating system has to allow this. On macOS you grant Accessibility
(and Screen Recording, for screenshots) to the app that loads tinycomputer.
Without it, the accessibility interface usually returns an empty window
instead of an error, and that looks exactly like an app with no buttons. So
tinycomputer checks permissions first and answers `PERM_DENIED`, naming the
setting to change.

### Platforms

macOS is fully supported. Windows and Linux build and load, but the
desktop engine implements no Windows or Linux surfaces yet, so desktop reads answer
`PLATFORM_NOT_SUPPORTED`. The browser works anywhere Chrome runs.

## On the web: sight

Web pages have accessibility markup too, but most sites get it partly wrong.
One airline's destination list was nine rows that claimed to be text boxes,
pointed at labels that didn't exist, and took no typing. A flow tried to type
a city into them and ended up choosing Mumbai.

A person never reads the markup. They see what's drawn. So on the web,
tinycomputer runs a small script in the page called **sight** that reads the
page the way a person looks at it:

- **What's a control?** Real links, buttons, and fields, plus anything with a
  pointer cursor, a click handler, or a tab stop.
- **What's visible?** Hidden, zero-size, transparent, and disabled things are
  left out. A control below the fold is marked `offscreen`. One hidden behind
  a dialog is marked `covered`.
- **What's it called?** The words a person would read: the text on the
  button, or for a field, its label, the words beside or above it, or its
  placeholder. An icon with no words is named from its alt text, its tooltip,
  or telling class names like `close` or `search`.
- **What takes text?** Decided by what the element really is (a real input,
  a text area, an editable region), not the role the page claims.
- **One control per thing you see.** A link wrapping its own label, or two
  elements drawn in the same box, become one control.

Sight also drops ads and empty boxes. If it can't read a page (some embedded
frames, for example), tinycomputer falls back to the accessibility tree.

Browser refs work a little differently from the desktop. Sight gives each
element a mark that lasts as long as the element is on the page, so a ref
stays good even after the page redraws around it.

## The browser session

tinycomputer drives Chrome through the vendored `agent-browser` engine, linked
straight into the module, so there's no extra process or socket. Each task
gets its own session, and up to eight run at once.

A session can:

- start a fresh Chrome, hidden (headless) or visible (headed);
- attach to a Chrome you already have running. Booking sites often turn away
  a fresh automated browser but serve a person's own. When the task ends, it
  only disconnects and leaves your browser open;
- be limited to certain websites (`origins`).

## Two surfaces, one model

Whatever the source, tinycomputer turns what it read into the same thing: a
**screen**. A screen has:

- the controls you can act on (at most 254), each with a role, name, value,
  states, what it supports (click, type, expand, scroll), its position, and
  the labels of what it sits inside;
- the static text: headings, labels, status lines;
- what's in front: a window, a sheet, a dialog, or a pop-over.

The desktop and the browser are each a **surface**, and every surface offers
the same short list of actions: click, type text, check, uncheck, expand,
collapse, scroll, wait, and press a key. The decision loop is written once
against that list and never knows which surface it's on. A **workspace** joins
the desktop and the browser, so one task can move between them.

## Summarizing a busy screen

A crowded web page can have hundreds of controls. For the wide strategy (see
[How it decides](how-it-decides.md)), tinycomputer builds a digest of the
screen:

- what's in front comes first;
- controls are grouped into regions;
- a list of search results shows one line per result;
- noise and distractions fold into one summary line;
- an unnamed control is described by the named thing it sits inside, like
  `combobox in button "destinationCity"`.

The digest is kept under a size budget, so even a huge page fits in one
question.

## The cursor you can watch

Clicks through the accessibility interface are invisible. Things just change.
To make a run easy to follow, tinycomputer can draw a second cursor on the
screen. It glides to each control just before it's pressed, with a curved,
slightly overshooting path like a real hand, and pulses when it lands.

It's purely cosmetic. It sends no input, never moves your own pointer, and
injects nothing into pages. Set its speed with the `cursor` configuration
(`off`, `brisk`, `natural`, or `calm`). It draws on macOS and Windows.

## Where to find out more

- [`technical/specs/browser-sight.md`](technical/specs/browser-sight.md): sight
  in full.
- [`technical/architecture.md`](technical/architecture.md#one-surface-abstraction):
  the surface abstraction.
- [`technical/specs/virtual-cursor.md`](technical/specs/virtual-cursor.md): the
  cursor.
- [The desktop crate docs](crates/tinycomputer-desktop/README.md) and
  [the browser crate docs](crates/tinycomputer-browser/README.md).
