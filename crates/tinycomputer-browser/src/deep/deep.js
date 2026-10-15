// Controls a page keeps inside open shadow roots, where the accessibility
// tree gives them no ref: a plain `span` or `div` a component wires to a
// click and draws with a pointer, or an editable region. See `mod.rs` for
// why, and for the shapes of the replies.
//
// Called as `(op, options) => reply`. `find` looks for the controls under
// `options.scope` (a CSS selector, or null for the whole page) and marks
// each with `data-tc-deep`. Every other `op` finds the one element carrying
// the mark `options.number` and acts on it or reads it, replying
// `{ gone: true }` when no element carries that mark, or more than one does.
((op, options) => {
  const MARK = 'data-tc-deep';
  // The most elements one look walks: a page larger than this is read no
  // further.
  const BUDGET = 50000;
  const squash = (text) => (text || '').replace(/[​-‍⁠﻿]/g, '')
    .replace(/\s+/g, ' ').trim();
  // Up the tree the page is drawn from: a slotted element to the slot that
  // shows it, a shadow root's child to its host.
  const up = (node) => node.assignedSlot || node.parentElement
    || (node.parentNode && node.parentNode.host) || null;
  // The elements drawn inside `node`: an open shadow root's children in
  // place of a host's own, and what a slot shows in place of the slot. A
  // closed root cannot be read, and its host's own children are walked.
  const kids = (node) => {
    if (node.shadowRoot) return node.shadowRoot.children;
    if (node.localName === 'slot') {
      const shown = node.assignedElements();
      return shown.length ? shown : node.children;
    }
    return node.children;
  };
  const drawn = (element) => {
    let area = false;
    for (const rect of element.getClientRects()) {
      if (rect.width > 0 && rect.height > 0) {
        area = true;
        break;
      }
    }
    return area && (!element.checkVisibility
      || element.checkVisibility({ opacityProperty: true, visibilityProperty: true }));
  };
  // Hidden from people as the tree hides it: inside `hidden`, `inert`, or
  // `aria-hidden` content.
  const veiled = (element) => element.hidden || element.inert
    || element.getAttribute('aria-hidden') === 'true';
  const hidden = (element) => {
    for (let at = element; at; at = up(at)) if (veiled(at)) return true;
    return false;
  };
  // The text drawn inside `element`, through the shadow roots its own
  // `innerText` never enters: up to `most` characters, a space around each
  // block.
  const SILENT = new Set(['style', 'script', 'template', 'noscript']);
  const drawnText = (element, most) => {
    let said = '';
    const stack = [element];
    while (stack.length && said.length < most) {
      const node = stack.pop();
      if (node === ' ') {
        said += ' ';
      } else if (node.nodeType === Node.TEXT_NODE) {
        said += node.data;
      } else if (node.nodeType === Node.ELEMENT_NODE && !SILENT.has(node.localName)) {
        const display = node === element ? 'inline' : getComputedStyle(node).display;
        if (display === 'none') continue;
        const block = !['inline', 'contents'].includes(display);
        const assigned = node.localName === 'slot' ? node.assignedNodes() : [];
        const children = node.shadowRoot ? node.shadowRoot.childNodes
          : assigned.length ? assigned : node.childNodes;
        if (block) stack.push(' ');
        for (let index = children.length - 1; index >= 0; index -= 1) stack.push(children[index]);
        if (block) stack.push(' ');
      }
    }
    return said;
  };
  // The words an element shows: its rendered text; for one whose words are
  // all drawn by components inside it, the text drawn there; else its label
  // or title.
  const words = (element, most) => squash(element.innerText)
    || squash(drawnText(element, most * 2))
    || squash(element.getAttribute('aria-label'))
    || squash(element.getAttribute('title'));

  // Marks and lists the controls, in the order the page draws them.
  const find = ({ scope, next, keep, most, longest }) => {
    // What agent-browser's snapshot gives a ref of its own: a native
    // control, a role that is one, and a role it refs once named. Such an
    // element is in the tree already, and so is whatever a person presses
    // inside a link or a button.
    const INTERACTIVE = new Set(['button', 'link', 'textbox', 'checkbox', 'radio', 'combobox',
      'listbox', 'menuitem', 'menuitemcheckbox', 'menuitemradio', 'option', 'searchbox', 'slider',
      'spinbutton', 'switch', 'tab', 'treeitem']);
    const NAMED = new Set(['heading', 'cell', 'gridcell', 'columnheader', 'rowheader', 'listitem',
      'article', 'region', 'main', 'navigation']);
    const BY_CONTENT = new Set(['heading', 'cell', 'gridcell', 'columnheader', 'rowheader']);
    const IMPLICIT = {
      h1: 'heading', h2: 'heading', h3: 'heading', h4: 'heading', h5: 'heading', h6: 'heading',
      td: 'cell', th: 'columnheader', li: 'listitem', article: 'article', main: 'main', nav: 'navigation',
    };
    const PRESSED_WITH = 'a[href], button, [role=button], [role=link], [role=menuitem], '
      + '[role=option], [role=tab], [role=checkbox], [role=radio], [role=switch]';
    const role = (element) => (element.getAttribute('role') || '').trim().toLowerCase().split(/\s+/)[0]
      || IMPLICIT[element.localName] || '';
    const inTree = (element) => {
      const tag = element.localName;
      if (['button', 'select', 'textarea', 'iframe'].includes(tag)) return true;
      if (tag === 'a') return element.hasAttribute('href');
      if (tag === 'input') return element.type !== 'hidden';
      const named = role(element);
      if (INTERACTIVE.has(named)) return true;
      return NAMED.has(named) && Boolean(squash(element.getAttribute('aria-label'))
        || element.hasAttribute('aria-labelledby')
        || (BY_CONTENT.has(named) && squash(element.textContent)));
    };
    const pointers = new Map();
    const pointer = (element) => {
      if (!pointers.has(element)) pointers.set(element, getComputedStyle(element).cursor === 'pointer');
      return pointers.get(element);
    };
    // How an element responds: `editable`, the outermost element of an
    // editable region; `clickable`, a click handler of its own, or a
    // pointer its parent does not show (a pointer inherited from a row is
    // the row's); `focusable`, a tab stop. Null for none of these: a click
    // listener added by script, with no pointer, cannot be seen from here.
    const kindOf = (element) => {
      const parent = up(element);
      if (element.isContentEditable) return parent && parent.isContentEditable ? null : 'editable';
      if (element.hasAttribute('onclick') || typeof element.onclick === 'function') return 'clickable';
      if (pointer(element) && !(parent && pointer(parent))) return 'clickable';
      if (element.hasAttribute('tabindex') && element.tabIndex >= 0) return 'focusable';
      return null;
    };
    let base = document.documentElement;
    if (scope) {
      try {
        base = document.querySelector(scope);
      } catch (error) {
        base = null;
      }
    }
    if (!base) return { found: [], next };
    const picks = [];
    const picked = new Set();
    // Every element still marked: a mark not handed out again comes off.
    const marked = [];
    const stack = [[document.documentElement, false]];
    let walked = 0;
    while (stack.length && walked < BUDGET) {
      const [element, outer] = stack.pop();
      walked += 1;
      if (element.hasAttribute(MARK)) marked.push(element);
      const inside = outer || element === base;
      const children = kids(element);
      for (let index = children.length - 1; index >= 0; index -= 1) stack.push([children[index], inside]);
      // Only what a shadow root holds: the tree reaches the rest itself.
      if (!inside || !(element.getRootNode() instanceof ShadowRoot) || inTree(element)) continue;
      const kind = kindOf(element);
      if (!kind || veiled(element) || !drawn(element)) continue;
      // The outermost of nested controls, though a field inside a pressable
      // row is a field of its own; nothing inside a control the tree has,
      // or hidden from people.
      let kept = true;
      for (let at = up(element); at && kept; at = up(at)) {
        kept = !veiled(at) && !at.matches(PRESSED_WITH)
          && !(picked.has(at) && (kind !== 'editable' || at.isContentEditable));
      }
      if (!kept) continue;
      picks.push({ element, kind });
      picked.add(element);
    }
    // A control still marked with a number the last snapshot handed out
    // keeps it; any other gets the next number, never handed out before.
    const held = new Set(keep);
    const used = new Set();
    const shown = picks.slice(0, most);
    const found = shown.map(({ element, kind }) => {
      let number = Number(element.getAttribute(MARK));
      if (!held.has(number) || used.has(number)) {
        number = next;
        next += 1;
      }
      used.add(number);
      // Written only when it changes: a page's own observers see no
      // attribute set again to what it was.
      if (element.getAttribute(MARK) !== String(number)) element.setAttribute(MARK, String(number));
      const text = words(element, longest);
      return { n: number, kind, text: text.length > longest ? `${text.slice(0, longest - 1)}…` : text };
    });
    const listed = new Set(shown.map(({ element }) => element));
    for (const element of marked) {
      if (!listed.has(element)) element.removeAttribute(MARK);
    }
    return { found, next };
  };

  // One thing done with, or read from, the element marked `number`; with
  // `field`, an aim also says whether the element takes text or holds
  // something that does.
  const act = (what, { number, field }) => {
    const matches = [];
    const roots = [document];
    while (roots.length) {
      const root = roots.pop();
      for (const element of root.querySelectorAll('*')) {
        if (element.shadowRoot) roots.push(element.shadowRoot);
        if (element.getAttribute(MARK) === String(number)) matches.push(element);
      }
    }
    if (matches.length !== 1) return { gone: true };
    const element = matches[0];
    const holds = (node) => {
      for (let at = node; at; at = up(at)) if (at === element) return true;
      return false;
    };
    // What holds the keyboard focus, inside open shadow roots.
    const focused = () => {
      let at = document.activeElement;
      while (at && at.shadowRoot && at.shadowRoot.activeElement) at = at.shadowRoot.activeElement;
      return at === document.body || at === document.documentElement ? null : at;
    };
    const where = (at) => ({ inside: Boolean(at) && holds(at), on: at ? at.localName : 'nothing' });
    const TEXT_TYPES = ['text', 'search', 'email', 'tel', 'url', 'number', 'password'];
    const takesText = (at) => {
      if (at.localName === 'textarea') return !at.readOnly && !at.disabled;
      if (at.localName === 'input') {
        return TEXT_TYPES.includes((at.getAttribute('type') || 'text').toLowerCase())
          && !at.readOnly && !at.disabled;
      }
      return at.isContentEditable === true;
    };
    switch (what) {
      case 'aim': {
        // Into the window when any of it lies outside, then the middle of
        // its largest box the window shows, and what a press there reaches.
        if (!drawn(element) || hidden(element)) return { unseen: true };
        const whole = element.getBoundingClientRect();
        if (whole.top < 0 || whole.left < 0 || whole.bottom > innerHeight || whole.right > innerWidth) {
          element.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
        }
        let best = null;
        for (const rect of element.getClientRects()) {
          const left = Math.max(rect.left, 0);
          const top = Math.max(rect.top, 0);
          const right = Math.min(rect.right, innerWidth);
          const bottom = Math.min(rect.bottom, innerHeight);
          const area = Math.max(0, right - left) * Math.max(0, bottom - top);
          if (area > 0 && (!best || area > best.area)) {
            best = { area, x: (left + right) / 2, y: (top + bottom) / 2 };
          }
        }
        if (!best) return { unseen: true };
        let hit = document.elementFromPoint(best.x, best.y);
        while (hit && hit.shadowRoot) {
          const inner = hit.shadowRoot.elementFromPoint(best.x, best.y);
          if (!inner || inner === hit) break;
          hit = inner;
        }
        let takes = false;
        if (field) {
          const stack = [element];
          for (let walked = 0; stack.length && !takes && walked < BUDGET; walked += 1) {
            const at = stack.pop();
            takes = takesText(at);
            const children = kids(at);
            for (let index = children.length - 1; index >= 0; index -= 1) stack.push(children[index]);
          }
        }
        if (hit && holds(hit)) return { x: best.x, y: best.y, field: takes };
        let cover = hit ? hit.localName : 'nothing';
        if (hit && hit.id) cover += `#${hit.id}`;
        else if (hit && typeof hit.className === 'string' && hit.className.trim()) {
          cover += `.${hit.className.trim().split(/\s+/).slice(0, 2).join('.')}`;
        }
        return { x: best.x, y: best.y, field: takes, cover };
      }
      case 'focus':
        element.focus();
        return where(focused());
      case 'end':
      case 'select': {
        // The field the press focused, with its caret put after what it
        // holds (`end`, to type) or all it holds selected (`select`, to
        // replace it).
        const at = focused();
        const place = where(at);
        if (!place.inside) return place;
        if (!takesText(at)) return { ...place, takes: false };
        if (at.localName === 'input' || at.localName === 'textarea') {
          if (what === 'select') {
            at.select();
          } else {
            try {
              at.setSelectionRange(at.value.length, at.value.length);
            } catch (error) {
              // A field with no caret of its own (`email`, `number`) keeps
              // it where the press put it.
            }
          }
        } else {
          let region = at;
          while (up(region) && up(region).isContentEditable) region = up(region);
          const range = document.createRange();
          range.selectNodeContents(region);
          if (what === 'end') range.collapse(false);
          const selection = getSelection();
          selection.removeAllRanges();
          selection.addRange(range);
        }
        return { ...place, takes: true };
      }
      case 'text':
        return { text: element.innerText || squash(drawnText(element, 100000)) };
      case 'visible':
        return { visible: drawn(element) && !hidden(element) };
      default:
        return { gone: true };
    }
  };

  return op === 'find' ? find(options) : act(op, options);
})
