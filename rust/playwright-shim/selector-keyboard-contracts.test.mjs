import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { applyKeyboardDefault, keyboardDefaultsSource } from "./keyboard-defaults.mjs";
import { queryVisibleSelector } from "./visible-selector.mjs";

// The adapter is the selector's supported seam: the algorithm owns relationships
// and extended pseudos; this fixture owns only native atomic CSS predicates.
function selectorTree() {
  const item = (id, tag, visible = true, ...children) => ({ id, tag, visible, children });
  const hidden = item("hidden", "span", false);
  const first = item("first", "section", true, item("leaf", "span"), hidden);
  const middle = item("middle", "aside");
  const last = item("last", "section", true, item("end", "span"));
  const root = item("root", "main", true, first, middle, last);
  const all = (node) => node.children.flatMap((child) => [child, ...all(child)]);
  for (const parent of [root, ...all(root)]) {
    parent.children.forEach((child, index) => {
      child.parent = parent;
      child.previous = parent.children[index - 1] ?? null;
    });
  }
  const calls = [];
  const atom = (node, css) => {
    calls.push(css);
    if (css === ":broken" || css === ":broken(") throw new SyntaxError("invalid native selector");
    if (css === ":fatal") throw new TypeError("native matcher failed");
    if (css === "*" || css === node.tag || css === `#${node.id}`) return true;
    return css === '[data-label="a,b > :visible"]' && node === middle;
  };
  const dom = {
    parent: (node) => node.parent ?? null,
    previous: (node) => node.previous ?? null,
    all,
    matches: atom,
    contains: (context, node) => all(context).includes(node),
    validate: (_, source) => {
      if (source === ":broken") throw new SyntaxError("invalid native selector");
    },
  };
  const query = (source, context = root) =>
    queryVisibleSelector(context, source, (node) => node.visible, dom).map((node) => node.id);
  return { root, first, middle, last, hidden, dom, query, calls };
}

test("extended selectors preserve order, group uniqueness and descendant boundaries", () => {
  const { query, first } = selectorTree();
  assert.deepEqual(query("section:visible > span:visible, #leaf:visible"), ["leaf", "end"]);
  assert.deepEqual(query("main:visible span:visible"), []);
  assert.deepEqual(query(":scope > span:visible", first), ["leaf"]);
  assert.deepEqual(query("span:visible", first), ["leaf"]);
  assert.deepEqual(query("section:visible > span:not(:visible)"), ["hidden"]);
});

test("adjacent and general siblings require the right relation and scoped parent", () => {
  const { query, first, last } = selectorTree();
  assert.deepEqual(query("section:visible + aside:visible"), ["middle"]);
  assert.deepEqual(query("section:visible + section:visible"), []);
  assert.deepEqual(query("section:visible ~ section:visible"), ["last"]);
  assert.deepEqual(query(":scope + aside:visible", first), ["middle"]);
  assert.deepEqual(query(":scope ~ section:visible", first), ["last"]);
  assert.deepEqual(query(":scope + aside:visible", last), []);
});

test("relational has scopes children and following siblings without mutating nodes", () => {
  const { query, root, first, middle, last, hidden } = selectorTree();
  const before = [root.children.slice(), first.children.slice(), hidden.visible];
  assert.deepEqual(query("section:has(> span:not(:visible))"), ["first"]);
  assert.deepEqual(query("section:has(+ aside:visible)"), ["first"]);
  assert.deepEqual(query("section:has(~ section:visible)"), ["first"]);
  assert.deepEqual(query("section:has(> aside:visible, > #end:visible)"), ["last"]);
  assert.deepEqual(root.children, before[0]);
  assert.deepEqual(first.children, before[1]);
  assert.equal(hidden.visible, before[2]);
  assert.equal(middle.previous, first);
  assert.equal(last.previous, middle);
});

test("is and where forgive syntax errors while not and matcher faults propagate", () => {
  const { query } = selectorTree();
  assert.deepEqual(query("section:is(:broken, #last:visible)"), ["last"]);
  assert.deepEqual(query("section:where(:broken, :not(:has(> #hidden:visible)))"), [
    "first",
    "last",
  ]);
  assert.throws(() => query("section:not(:broken)"), SyntaxError);
  assert.throws(() => query("section:is(:fatal, #first:visible)"), TypeError);
  assert.throws(() => query("section:has(> span:visible"), /Unclosed selector/);
  assert.throws(() => query(":broken"), SyntaxError);
});

test("quoted commas, combinators and pseudo text remain native attribute content", () => {
  const { query, calls } = selectorTree();
  assert.deepEqual(query('[data-label="a,b > :visible"]:visible'), ["middle"]);
  assert.ok(calls.includes('[data-label="a,b > :visible"]'));
  assert.deepEqual(query("section/* comment > , */:visible > span:visible"), ["leaf", "end"]);
  assert.deepEqual(query("section:visible/* unfinished"), ["first", "last"]);
});

test("native-only queries scope each element branch but preserve document selectors", () => {
  const requests = [];
  const element = { nodeType: 1, querySelectorAll: (css) => (requests.push(css), ["result"]) };
  const document = { nodeType: 9, querySelectorAll: element.querySelectorAll };
  assert.deepEqual(
    queryVisibleSelector(element, "span, :scope > aside", () => true),
    ["result"],
  );
  assert.equal(requests.pop(), ":scope span, :scope > aside");
  queryVisibleSelector(document, "span, aside", () => true);
  assert.equal(requests.pop(), "span, aside");
});

test("escaped CSS punctuation, hex whitespace and quoted escapes stay in native atoms", () => {
  const { root, middle, dom } = selectorTree();
  const atoms = [String.raw`.name\,part`, ".\\000061\r\nb", String.raw`[title="a\" > , b"]`];
  for (const atom of atoms) {
    const seen = [];
    const adapter = {
      ...dom,
      matches: (node, css) => (seen.push(css), node === middle && css === atom),
    };
    const result = queryVisibleSelector(root, `${atom}:visible`, (node) => node.visible, adapter);
    assert.deepEqual(result, [middle]);
    assert.ok(seen.every((css) => css === atom));
  }
});

test("ordinary DOM traversal uses element parents, siblings and native matches without an adapter", () => {
  const { root, first, middle, last, dom } = selectorTree();
  for (const node of [root, ...dom.all(root)]) {
    node.parentElement = node.parent ?? null;
    node.previousElementSibling = node.previous ?? null;
    node.querySelectorAll = () => dom.all(node);
    node.matches = (css) => dom.matches(node, css);
  }
  const query = (context, css) => queryVisibleSelector(context, css, (node) => node.visible);
  assert.deepEqual(query(root, "section:visible + aside:visible"), [middle]);
  assert.deepEqual(query(root, "section:visible ~ section:visible"), [last]);
  assert.deepEqual(query(first, ":scope > span:visible"), [first.children[0]]);
  const document = {
    nodeType: 9,
    documentElement: root,
    querySelectorAll: () => [root, ...dom.all(root)],
  };
  assert.deepEqual(query(document, ":scope:visible > section:visible"), [first, last]);
  assert.deepEqual(query(root, ":scope + section:visible"), []);
});

function keyboardControl(value = "", type = "text", serialized = false) {
  const events = [];
  class Input extends EventTarget {
    tagName = "INPUT";
    disabled = false;
    readOnly = false;
    constructor() {
      super();
      this.value = value;
      this.type = type;
      this.selectionStart = this.selectionEnd = value.length;
      for (const name of ["beforeinput", "input"])
        this.addEventListener(name, (event) =>
          events.push([name, event.inputType, event.data, this.value]),
        );
    }
    setSelectionRange(start, end) {
      this.selectionStart = start;
      this.selectionEnd = end;
    }
    select() {
      this.setSelectionRange(0, this.value.length);
    }
  }
  class InputEvent extends Event {
    constructor(name, options) {
      super(name, options);
      this.inputType = options.inputType;
      this.data = options.data;
    }
  }
  let apply = applyKeyboardDefault;
  if (serialized) {
    const realm = vm.createContext({ InputEvent });
    vm.runInContext(keyboardDefaultsSource(), realm);
    apply = realm.applyKeyboardDefault;
  }
  const input = new Input();
  const press = (key, options = {}, hooks = {}) => {
    const event = { key, ...options };
    const prior = Object.getOwnPropertyDescriptor(globalThis, "InputEvent");
    Object.defineProperty(globalThis, "InputEvent", { configurable: true, value: InputEvent });
    try {
      for (const phase of ["keydown", "keyup"])
        apply(input, event, phase, hooks.replace, hooks.submit, hooks.click);
    } finally {
      if (prior) Object.defineProperty(globalThis, "InputEvent", prior);
      else delete globalThis.InputEvent;
    }
  };
  return { input, events, press };
}

test("serialized keyboard text edits preserve surrogate pairs, selections and input order", () => {
  const { input, events, press } = keyboardControl("a😀b");
  input.setSelectionRange(3, 3);
  press("Backspace");
  assert.equal(input.value, "ab");
  assert.equal(input.selectionStart, 1);
  press("😀");
  assert.equal(input.value, "a😀b");
  assert.equal(input.selectionStart, 3);
  input.setSelectionRange(1, 1);
  press("Delete");
  assert.equal(input.value, "ab");
  assert.deepEqual(events.slice(0, 2), [
    ["beforeinput", "deleteContentBackward", null, "a😀b"],
    ["input", "deleteContentBackward", null, "ab"],
  ]);
  input.setSelectionRange(0, 2);
  press("z");
  assert.equal(input.value, "z");
  assert.equal(input.selectionEnd, 1);
});

test("cancelled, readonly, disabled and modified keystrokes cannot change text", () => {
  for (const attribute of ["disabled", "readOnly"]) {
    const { input, events, press } = keyboardControl("stay");
    input[attribute] = true;
    press("x");
    assert.equal(input.value, "stay");
    assert.deepEqual(events, []);
  }
  const { input, events, press } = keyboardControl("stay");
  for (const modifier of ["ctrlKey", "metaKey", "altKey"]) press("x", { [modifier]: true });
  assert.deepEqual(events, []);
  input.addEventListener("beforeinput", (event) => event.preventDefault());
  press("x");
  assert.equal(input.value, "stay");
  assert.deepEqual(events, [["beforeinput", "insertText", "x", "stay"]]);
});

test("select-all then replacement uses selection and empty deletion emits no input", () => {
  const { input, events, press } = keyboardControl("replace");
  press("a", { ctrlKey: true });
  assert.deepEqual([input.selectionStart, input.selectionEnd], [0, 7]);
  press("ok");
  assert.equal(input.value, "replace");
  press("z");
  assert.equal(input.value, "z");
  input.value = "";
  input.setSelectionRange(0, 0);
  events.length = 0;
  press("Delete");
  assert.deepEqual(events, [["beforeinput", "deleteContentForward", null, ""]]);
});

test("activation occurs on Enter-down or Space-up and requires the native hook", () => {
  const { input, press } = keyboardControl("", "checkbox");
  const activations = [];
  press(" ", {}, { click: (element) => activations.push(element) });
  assert.deepEqual(activations, [input]);
  press("Enter", {}, { submit: () => activations.push("submit") });
  assert.deepEqual(activations, [input, "submit"]);
  input.type = "button";
  assert.throws(() => press("Enter"), /Native keyboard activation is unavailable/);
  input.disabled = true;
  press(" ");
  assert.deepEqual(activations, [input, "submit"]);
});

test("serialized defaults run without module bindings", () => {
  const { input, events, press } = keyboardControl("abc", "text", true);
  press("Backspace");
  assert.equal(input.value, "ab");
  assert.equal(events[1][1], "deleteContentBackward");
});
