// document.createTreeWalker + NodeFilter — focus-management code (MUI's DataGrid / focus
// trap, ARIA widgets) walks the tree with these; deno_core's binding has neither, so a
// page with a data grid threw "createTreeWalker is not a function" and rendered blank.
// A document-order DFS honoring whatToShow + the accept filter (REJECT skips the subtree,
// SKIP skips the node but descends) — enough for focusable-element scans.
(() => {
  const d = globalThis.document;
  if (!d || typeof d.createTreeWalker === "function") return;
  globalThis.NodeFilter = globalThis.NodeFilter || {
    SHOW_ALL: 0xffffffff,
    SHOW_ELEMENT: 1,
    SHOW_TEXT: 4,
    SHOW_COMMENT: 128,
    FILTER_ACCEPT: 1,
    FILTER_REJECT: 2,
    FILTER_SKIP: 3,
  };
  const ACCEPT = 1,
    REJECT = 2;
  // createTreeWalker(root, optional whatToShow, optional filter).length === 1.
  d.createTreeWalker = function (root) {
    const whatToShow = arguments[1],
      filter = arguments[2];
    const show = whatToShow == null ? 0xffffffff : whatToShow >>> 0;
    const accept = (n) => {
      const t = n.nodeType || 1;
      const bit = t === 1 ? 1 : t === 3 ? 4 : t === 8 ? 128 : 1 << (t - 1);
      if ((show & bit) === 0) return 3; // SKIP (wrong type)
      const fn = filter && (typeof filter === "function" ? filter : filter.acceptNode);
      if (typeof fn === "function") {
        try {
          return fn.call(filter, n);
        } catch {
          return 1;
        }
      }
      return 1;
    };
    const w = { root, whatToShow: show, filter, currentNode: root };
    const move = (value) => {
      if (value) w.currentNode = value;
      return value;
    };
    // Search a filtered child axis without crossing its starting parent.
    const child = (parent, first, sibling) => {
      let candidate = parent[first];
      while (candidate) {
        const result = accept(candidate);
        if (result === ACCEPT) return candidate;
        if (result !== REJECT && candidate[first]) {
          candidate = candidate[first];
          continue;
        }
        while (candidate && candidate !== parent && !candidate[sibling])
          candidate = candidate.parentNode;
        if (!candidate || candidate === parent) return null;
        candidate = candidate[sibling];
      }
      return null;
    };
    const sibling = (first, axis) => {
      let candidate = w.currentNode;
      while (candidate && candidate !== root) {
        while (candidate[axis]) {
          candidate = candidate[axis];
          const result = accept(candidate);
          if (result === ACCEPT) return candidate;
          if (result !== REJECT) {
            const nested = child(candidate, first, axis);
            if (nested) return nested;
          }
        }
        candidate = candidate.parentNode;
        if (!candidate || candidate === root || accept(candidate) === ACCEPT) return null;
      }
      return null;
    };
    w.firstChild = () => move(child(w.currentNode, "firstChild", "nextSibling"));
    w.lastChild = () => move(child(w.currentNode, "lastChild", "previousSibling"));
    w.nextSibling = () => move(sibling("firstChild", "nextSibling"));
    w.previousSibling = () => move(sibling("lastChild", "previousSibling"));
    w.parentNode = () => {
      let candidate = w.currentNode;
      while (candidate && candidate !== root) {
        candidate = candidate.parentNode;
        if (candidate && accept(candidate) === ACCEPT) return move(candidate);
      }
      return null;
    };
    w.nextNode = () => {
      let candidate = w.currentNode;
      let descend = true;
      while (candidate) {
        if (descend && candidate.firstChild) candidate = candidate.firstChild;
        else {
          while (candidate && candidate !== root && !candidate.nextSibling)
            candidate = candidate.parentNode;
          if (!candidate || candidate === root) return null;
          candidate = candidate.nextSibling;
        }
        const result = accept(candidate);
        if (result === ACCEPT) return move(candidate);
        descend = result !== REJECT;
      }
      return null;
    };
    w.previousNode = () => {
      let candidate = w.currentNode;
      while (candidate && candidate !== root) {
        if (!candidate.previousSibling) {
          candidate = candidate.parentNode;
          if (candidate && accept(candidate) === ACCEPT) return move(candidate);
          continue;
        }
        candidate = candidate.previousSibling;
        let result = accept(candidate);
        while (result !== REJECT && candidate.lastChild) {
          candidate = candidate.lastChild;
          result = accept(candidate);
        }
        if (result === ACCEPT) return move(candidate);
      }
      return null;
    };
    return w;
  };
  // NodeIterator (same filter model, linear) — some libs use it instead of TreeWalker.
  if (typeof d.createNodeIterator !== "function") {
    d.createNodeIterator = function (root) {
      const whatToShow = arguments[1],
        filter = arguments[2];
      const tw = d.createTreeWalker(root, whatToShow, filter);
      return { nextNode: () => tw.nextNode(), previousNode: () => null, detach() {} };
    };
  }
})();
