import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { makeKeyboard } from "./keyboard.mjs";
import { queryVisibleSelector } from "./visible-selector.mjs";

function nativeAtoms() {
  const leaf = { tag: "leaf", parentElement: null };
  const parent = { tag: "parent", parentElement: null };
  const root = { nodeType: 1, tag: "root", parentElement: null };
  leaf.parentElement = parent;
  parent.parentElement = root;
  const atoms = [];
  for (const node of [root, parent, leaf]) {
    node.querySelectorAll = () => (node === root ? [parent, leaf] : node === parent ? [leaf] : []);
    node.matches = (css) => {
      atoms.push(css);
      if (css === "*" || css === node.tag) return true;
      if (css.includes(":visible(") || css.endsWith(":not") || css.endsWith('"'))
        throw new SyntaxError("Invalid native atom");
      return css === ":is(native)" && node === leaf;
    };
  }
  const query = (source) => queryVisibleSelector(root, source, () => true);
  return { query, atoms, leaf };
}

test("extended ancestor matching reaches a nested descendant without matching its root", () => {
  const { query, leaf } = nativeAtoms();
  assert.deepEqual(query("parent leaf:visible"), [leaf]);
  assert.deepEqual(query("root leaf:visible"), []);
});

test("nonextended pseudos stay native while nested visible and scope use extended semantics", () => {
  const { query, leaf, atoms } = nativeAtoms();
  assert.deepEqual(query(":is(native):visible"), [leaf]);
  assert.ok(atoms.includes(":is(native)"));
  assert.deepEqual(query("leaf:is(:visible)"), [leaf]);
  assert.deepEqual(query("leaf:not(:scope):visible"), [leaf]);
  assert.deepEqual(query("parent:has(leaf:visible)"), [leaf.parentElement]);
  assert.throws(() => query("leaf:visible()"), SyntaxError);
  assert.throws(() => query("leaf:not:visible"), SyntaxError);
});

test("malformed quoted selectors propagate native syntax failures instead of producing matches", () => {
  const { query } = nativeAtoms();
  assert.throws(() => query('leaf:visible[title="'), SyntaxError);
});

test("hex escapes and quoted pseudo arguments preserve opaque native atoms", () => {
  const { query, atoms } = nativeAtoms();
  for (const source of [
    String.raw`\61:visible`,
    "\\61 \t:visible",
    ".\\a:visible",
    ':is([title="x,y"]):visible',
  ]) {
    assert.deepEqual(query(source), []);
  }
  assert.ok(atoms.includes(String.raw`\61`));
  assert.ok(atoms.includes(':is([title="x,y"])'));
});

function keyboardTransport() {
  const events = [];
  const body = new EventTarget();
  body.tagName = "DIV";
  body.isContentEditable = false;
  class KeyboardEvent extends Event {
    constructor(name, options) {
      super(name, options);
      Object.assign(this, { key: options.key, code: options.code, location: options.location });
    }
  }
  for (const name of ["keydown", "keyup", "keypress"])
    body.addEventListener(name, (event) =>
      events.push([name, event.key, event.code, event.location]),
    );
  const realm = vm.createContext({ document: { activeElement: body }, KeyboardEvent, __tsNs: {} });
  const page = {
    _ensureLive: async () => {},
    _sessionEval: async (source, options) => {
      assert.deepEqual(options, { waitForBindings: false });
      vm.runInContext(source, realm);
      return realm.__RESULT;
    },
  };
  return { keyboard: makeKeyboard(page), events };
}

test("keyboard aliases and physical key names generate platform-aware semantic events", async () => {
  const { keyboard, events } = keyboardTransport();
  await keyboard.press("ControlOrMeta+KeyQ");
  const modifier = process.platform === "darwin" ? "Meta" : "Control";
  assert.deepEqual(
    events.map((event) => event.slice(0, 3)),
    [
      ["keydown", modifier, `${modifier}Left`],
      ["keydown", "q", "KeyQ"],
      ["keyup", "q", "KeyQ"],
      ["keyup", modifier, `${modifier}Left`],
    ],
  );
  events.length = 0;
  await keyboard.press("Digit7");
  assert.deepEqual(
    events.map((event) => event.slice(0, 3)),
    [
      ["keydown", "7", "Digit7"],
      ["keypress", "7", "Digit7"],
      ["keyup", "7", "Digit7"],
    ],
  );
});

test("right-hand modifiers carry right location and a literal plus is a single key", async () => {
  const { keyboard, events } = keyboardTransport();
  await keyboard.down("ControlRight");
  await keyboard.up("ControlRight");
  assert.deepEqual(events, [
    ["keydown", "Control", "ControlRight", 2],
    ["keyup", "Control", "ControlRight", 2],
  ]);
  events.length = 0;
  await keyboard.press("+");
  assert.deepEqual(events, [
    ["keydown", "+", "+", 0],
    ["keypress", "+", "+", 0],
    ["keyup", "+", "+", 0],
  ]);
});
