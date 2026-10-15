// Controls a page keeps inside open shadow roots, where the accessibility
// tree gives them no ref: a plain `span` or `div` a component wires to a
// click and draws with a pointer, or an editable region. See `mod.rs` for
// why, and for the shapes of the replies.
//
// Called as `(op, options) => reply`. `find` looks for the controls under
// `options.scope` (a CSS selector, or null for the whole page) and marks
// each with `data-tc-deep`. `pressed` says whether the last press went where
// it was aimed. Every other `op` finds the one element carrying the mark
// `options.number` and acts on it or reads it, replying `{ gone: true }`
// when no element carries that mark, more than one does, or it no longer
// shows the words its ref was handed out with (`options.words`, when given).
((op, options) => {
  const MARK = 'data-tc-deep';
  const GUARD = Symbol.for('tinycomputer.deep.guard');
  // The most elements one look walks, and one action searches for its mark,
  // and the most nodes read for one control's words: past these, a page is
  // read no further.
  const BUDGET = 50000;
  const SEARCH = 200000;
  const TEXT_NODES = 2000;
  // Invisible format characters (zero-width spaces and joiners) come out, as
  // agent-browser takes them out of the names it shows.
  const squash = (text) => (text || '').replace(/\p{Cf}/gu, '').replace(/\s+/g, ' ').trim();
  // Words as a control's are compared with the tree's lines: no spaces (the
  // tree drops those between the pieces of a row) and in lower case (it
  // shows text as drawn, `text-transform` and all).
  const keyOf = (text) => squash(text).replace(/\s+/g, '').toLowerCase();
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
  // The same, text included.
  const nodes = (node) => {
    if (node.shadowRoot) return node.shadowRoot.childNodes;
    if (node.localName === 'slot') {
      const shown = node.assignedNodes();
      return shown.length ? shown : node.childNodes;
    }
    return node.childNodes;
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
  // Left out of the tree altogether: hidden from people, or not drawn.
  const unshown = (element) => {
    if (veiled(element)) return true;
    const style = getComputedStyle(element);
    return style.display === 'none' || style.visibility === 'hidden';
  };

  // What agent-browser's snapshot gives a ref of its own: a native control,
  // a role that is one, and a role it refs once named. Such an element is in
  // the tree already, and so is whatever a person presses inside a link or a
  // button.
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
  // What takes a press or keys of its own: inside a control, a press there
  // is that control's, not the one aimed at.
  const CONTROLS = `${PRESSED_WITH}, input, select, textarea, summary, [onclick], `
    + `[contenteditable=""], [contenteditable="true"], [${MARK}]`;
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

  // The text drawn inside `element`, through the shadow roots its own
  // `innerText` never enters: up to `most` characters, a space around each
  // block.
  const SILENT = new Set(['style', 'script', 'template', 'noscript']);
  const drawnText = (element, most) => {
    let said = '';
    let read = 0;
    const stack = [element];
    while (stack.length && said.length < most && read < TEXT_NODES) {
      const node = stack.pop();
      read += 1;
      if (node === ' ') {
        said += ' ';
      } else if (node.nodeType === Node.TEXT_NODE) {
        said += node.data;
      } else if (node.nodeType === Node.ELEMENT_NODE && !SILENT.has(node.localName)) {
        const display = node === element ? 'inline' : getComputedStyle(node).display;
        if (display === 'none') continue;
        const block = !['inline', 'contents'].includes(display);
        const children = nodes(node);
        if (block) stack.push(' ');
        for (let index = children.length - 1; index >= 0; index -= 1) stack.push(children[index]);
        if (block) stack.push(' ');
      }
    }
    return said;
  };
  const label = (element) => squash(element.getAttribute('aria-label'))
    || squash(element.getAttribute('aria-placeholder')) || squash(element.getAttribute('title'));
  // The words a control shows, at most `longest` characters: an editable
  // region's label, since what was typed into it names nothing (and may be
  // private); any other's rendered text, else the text drawn by components
  // inside it, else its label.
  const words = (element, longest) => {
    const said = element.isContentEditable ? label(element)
      : squash(element.innerText) || squash(drawnText(element, longest * 2)) || label(element);
    return said.length > longest ? `${said.slice(0, longest - 1)}…` : said;
  };
  // Two drawn frames, or 100 ms in a tab that draws none.
  const frames = () => new Promise((resolve) => {
    const timer = setTimeout(resolve, 100);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      clearTimeout(timer);
      resolve();
    }));
  });
  // An element by tag, id, and class, as agent-browser names a cover.
  const describe = (node) => {
    let said = (node && node.localName) || 'nothing';
    if (node && node.id) said += `#${node.id}`;
    else if (node && typeof node.className === 'string' && node.className.trim()) {
      said += `.${node.className.trim().split(/\s+/).slice(0, 2).join('.')}`;
    }
    return said;
  };

  // Where each control's words show among the text of the page, counted as
  // the tree shows its lines (`merge.rs`): the outermost nodes whose whole
  // text is those words, leaving out what the tree leaves out (text hidden
  // from people, and text inside a control the tree names itself, which no
  // line of words around it can hold either). A control is the `seen`th of
  // `of` such places; `merge.rs` puts it there only when the tree shows its
  // words as many times. Live, a sold-out row's own "Select" came before the
  // listed ones, and placing each control on the next line with its words
  // put every later one a row too early.
  const places = (base, controls, longest) => {
    const keys = new Set();
    for (const control of controls) {
      if (control.text && control.text.length < longest) {
        control.key = keyOf(control.text);
        if (control.key) keys.add(control.key);
      }
    }
    const spots = new Map();
    if (!keys.size) return spots;
    const longestKey = Math.max(...[...keys].map((key) => key.length));
    // Each node's words without spaces, as `keyOf` makes them; null for a
    // node no line of words can stand for (it holds a control the tree
    // names itself) or with more words than any control's.
    const said = new Map();
    const stack = [{ node: base, children: null, at: 0, text: '' }];
    let walked = 0;
    while (stack.length) {
      const top = stack[stack.length - 1];
      if (top.children === null) {
        walked += 1;
        if (walked > BUDGET) return spots;
        const { node } = top;
        top.children = [];
        if (node.nodeType === Node.TEXT_NODE) top.text = keyOf(node.data);
        else if (node.nodeType !== Node.ELEMENT_NODE || SILENT.has(node.localName) || unshown(node)) top.text = '';
        else if (node !== base && inTree(node)) top.text = null;
        else top.children = nodes(node);
      }
      if (top.at < top.children.length) {
        stack.push({ node: top.children[top.at], children: null, at: 0, text: '' });
        top.at += 1;
        continue;
      }
      stack.pop();
      const text = top.text !== null && top.text.length <= longestKey ? top.text : null;
      said.set(top.node, text);
      const parent = stack[stack.length - 1];
      if (parent) parent.text = parent.text === null || text === null ? null : parent.text + text;
    }
    const shows = new Map();
    const order = [base];
    while (order.length) {
      const node = order.pop();
      const text = said.get(node);
      if (text === undefined) continue;
      if (text && keys.has(text)) {
        if (!shows.has(text)) shows.set(text, []);
        shows.get(text).push(node);
        continue;
      }
      const children = nodes(node);
      for (let index = children.length - 1; index >= 0; index -= 1) order.push(children[index]);
    }
    for (const control of controls) {
      const list = control.key ? shows.get(control.key) || [] : [];
      for (let at = control.element; at; at = up(at)) {
        const seen = list.indexOf(at);
        if (seen >= 0) {
          spots.set(control.element, { seen, of: list.length });
          break;
        }
        if (at === base) break;
      }
    }
    return spots;
  };

  // Marks and lists the controls, in the order the page draws them.
  const find = ({ scope, next, keep, most, longest }) => {
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
      // A label pressed for a field the tree has is that field.
      if (element.localName === 'label' && element.control && inTree(element.control)
        && !unshown(element.control)) continue;
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
    // A control keeps the number the last snapshot handed it while it
    // still shows the words it was handed out with: a node a page re-drew
    // in place to show something else gets the next number, never handed
    // out before, as does any control new to the page.
    const held = new Map(keep.map(({ n, text }) => [n, text]));
    const used = new Set();
    const shown = picks.slice(0, most).map(({ element, kind }) => {
      const text = words(element, longest);
      let number = Number(element.getAttribute(MARK));
      if (held.get(number) !== text || used.has(number)) {
        number = next;
        next += 1;
      }
      used.add(number);
      // Written only when it changes: a page's own observers see no
      // attribute set again to what it was.
      if (element.getAttribute(MARK) !== String(number)) element.setAttribute(MARK, String(number));
      return { element, kind, text, number };
    });
    const listed = new Set(shown.map(({ element }) => element));
    for (const element of marked) {
      if (!listed.has(element)) element.removeAttribute(MARK);
    }
    const spots = places(base, shown, longest);
    const found = shown.map(({ element, kind, text, number }) => ({
      n: number, kind, text, ...(spots.get(element) || {}),
    }));
    return { found, next };
  };

  // Whether the last press went where it was aimed: the guard an aim set
  // stops any part of a press that would reach something else.
  const pressed = () => {
    const guard = window[GUARD];
    if (!guard) return { missed: null };
    guard.remove();
    return { missed: guard.missed };
  };

  // One thing done with, or read from, the element marked `number`.
  const act = async (what, { number, field, words: shows, guard, longest }) => {
    const matches = [];
    const roots = [document];
    let searched = 0;
    while (roots.length && searched < SEARCH) {
      const root = roots.pop();
      for (const element of root.querySelectorAll('*')) {
        searched += 1;
        if (element.shadowRoot) roots.push(element.shadowRoot);
        if (element.getAttribute(MARK) === String(number)) matches.push(element);
      }
    }
    if (matches.length !== 1) return { gone: true };
    const element = matches[0];
    // A node the page re-drew in place to show something else (a list that
    // recycles its rows) is no longer the control its ref was handed out
    // for.
    if (typeof shows === 'string' && words(element, longest) !== shows) return { gone: true };
    const holds = (node) => {
      for (let at = node; at; at = up(at)) if (at === element) return true;
      return false;
    };
    const topAt = (x, y) => {
      let hit = document.elementFromPoint(x, y);
      while (hit && hit.shadowRoot) {
        const inner = hit.shadowRoot.elementFromPoint(x, y);
        if (!inner || inner === hit) break;
        hit = inner;
      }
      return hit;
    };
    // Where a press on the element goes: the middle of the largest part the
    // window shows of it, else the first point of a grid over it where what
    // is on top is the element, or something in it that is no control of
    // its own (or, for text, a field). A card holding a button of its own
    // is pressed beside the button, never on it.
    const SPOTS = [[0.5, 0.5], [0.25, 0.5], [0.75, 0.5], [0.5, 0.25], [0.5, 0.75], [0.1, 0.5],
      [0.9, 0.5], [0.25, 0.25], [0.75, 0.25], [0.25, 0.75], [0.75, 0.75]];
    const aimAt = () => {
      const parts = [...element.getClientRects()].map((rect) => {
        const left = Math.max(rect.left, 0);
        const top = Math.max(rect.top, 0);
        const right = Math.min(rect.right, innerWidth);
        const bottom = Math.min(rect.bottom, innerHeight);
        return { left, top, right, bottom, area: Math.max(0, right - left) * Math.max(0, bottom - top) };
      }).filter((part) => part.area > 0).sort((one, other) => other.area - one.area);
      if (!parts.length) return { unseen: true };
      let nested = null;
      let covered = null;
      for (const part of parts) {
        for (const [across, down] of SPOTS) {
          const x = part.left + (part.right - part.left) * across;
          const y = part.top + (part.bottom - part.top) * down;
          const hit = topAt(x, y);
          if (!hit || !holds(hit)) {
            if (!covered) covered = { x, y, cover: describe(hit) };
            continue;
          }
          // A field is what a press for text is meant to reach.
          let inner = null;
          for (let at = hit; at && at !== element && !inner; at = up(at)) {
            if (at.matches && at.matches(CONTROLS) && !(field && takesText(at))) inner = at;
          }
          if (!inner) return { x, y };
          if (!nested) nested = inner;
        }
      }
      return nested ? { nested: describe(nested) } : covered;
    };
    // Stops any part of the coming press (a trusted pointer or mouse event,
    // or the click) whose path misses the element: something moved over
    // it, or it moved, between the aim and the press. It goes once the
    // press is read (`pressed`), or after 3 s.
    const arm = () => {
      if (window[GUARD]) window[GUARD].remove();
      const TYPES = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click'];
      const watch = { missed: null, remove: null };
      const check = (event) => {
        if (!event.isTrusted || event.composedPath().includes(element)) return;
        if (!watch.missed) watch.missed = describe(event.composedPath()[0] || event.target);
        event.preventDefault();
        event.stopImmediatePropagation();
      };
      for (const type of TYPES) window.addEventListener(type, check, true);
      const timer = setTimeout(() => watch.remove(), 3000);
      watch.remove = () => {
        clearTimeout(timer);
        for (const type of TYPES) window.removeEventListener(type, check, true);
        if (window[GUARD] === watch) delete window[GUARD];
      };
      window[GUARD] = watch;
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
        if (!drawn(element) || hidden(element)) return { unseen: true };
        const whole = element.getBoundingClientRect();
        if (whole.top < 0 || whole.left < 0 || whole.bottom > innerHeight || whole.right > innerWidth) {
          element.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
        }
        // What a scroll sets off (a sticky header, rows loading, a slide)
        // runs first: a press goes only where the element has held still
        // for two frames.
        let still = false;
        for (let tries = 0; tries < 4 && !still; tries += 1) {
          const before = element.getBoundingClientRect();
          await frames();
          const after = element.getBoundingClientRect();
          still = ['left', 'top', 'width', 'height']
            .every((side) => Math.abs(before[side] - after[side]) <= 1);
        }
        if (!still) return { moving: true };
        if (!drawn(element) || hidden(element)) return { unseen: true };
        const spot = aimAt();
        if (spot.unseen || spot.nested || spot.cover !== undefined) return spot;
        // Whether it takes text, or holds something that does.
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
        if (guard && (takes || !field)) arm();
        return { x: spot.x, y: spot.y, field: takes };
      }
      case 'focus':
        element.focus();
        return where(focused());
      case 'end':
      case 'select': {
        // The press settles first: a popup that takes the focus a frame
        // later holds it when it is checked.
        await frames();
        // The field the press focused, with its caret put after what it
        // holds (`end`, to type) or all it holds selected (`select`, to
        // replace it).
        const at = focused();
        const place = where(at);
        if (!place.inside) return place;
        if (!takesText(at)) return { ...place, takes: false };
        let caret = true;
        if (at.localName === 'input' || at.localName === 'textarea') {
          if (what === 'select') {
            at.select();
          } else {
            try {
              at.setSelectionRange(at.value.length, at.value.length);
            } catch (error) {
              // A field with no caret to set (`email`, `number`): the End
              // key puts it after what the field holds.
              caret = false;
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
        return { ...place, takes: true, caret };
      }
      case 'text':
        return { text: element.innerText || squash(drawnText(element, 100000)) };
      case 'visible':
        return { visible: drawn(element) && !hidden(element) };
      default:
        return { gone: true };
    }
  };

  if (op === 'find') return find(options);
  if (op === 'pressed') return pressed();
  return act(op, options);
})
