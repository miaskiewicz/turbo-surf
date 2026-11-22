import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const runtime = readFileSync(
  new URL("../crates/turbo-surf-render/src/runtime.rs", import.meta.url),
  "utf8",
);
const start = runtime.indexOf("__G.tcGetBy = function");
const end = runtime.indexOf("\n};", start) + 3;
function fixture() {
  let outsideReads = 0;
  const button = (text, outside = false) => ({
    tagName: "BUTTON",
    getAttribute() {
      if (outside) outsideReads++;
      return null;
    },
    textContent: text,
    querySelectorAll() {
      return [];
    },
  });
  const inside = button("Inside");
  const outside = Array.from({ length: 100 }, () => button("Inside", true));
  const root = {
    tagName: "DIV",
    getAttribute() {
      return null;
    },
    textContent: "Inside",
    querySelectorAll() {
      return [inside];
    },
  };
  const all = [root, inside, ...outside];
  const document = {
    querySelectorAll(s) {
      return s === "*" ? all : s === "#scope" ? [root] : [];
    },
  };
  const context = vm.createContext({
    document,
    __G: {
      tcLocatorAll: () => all,
      tcLocatorQuery: (root, selector) => root.querySelectorAll(selector),
    },
    getComputedStyle: () => ({ visibility: "visible" }),
  });
  vm.runInContext(runtime.slice(start, end), context);
  return { context, outsideReads: () => outsideReads };
}

test("scoped live role matching never reads outside candidates", () => {
  const { context, outsideReads } = fixture();
  vm.runInContext("__G.tcGetBy('role', 'button', 'Inside', '#scope')", context);
  assert.deepEqual(JSON.parse(context.__RESULT), [{ idx: 1 }]);
  assert.equal(outsideReads(), 0);
});

test("live regex role names compile once and reset state for each candidate", () => {
  const { context } = fixture();
  vm.runInContext(
    `
    var compileCount = 0;
    var OriginalRegExp = RegExp;
    RegExp = function (...args) { compileCount++; return new OriginalRegExp(...args); };
    __G.tcGetBy('role', 'button', '/Inside/g');
  `,
    context,
  );
  assert.equal(JSON.parse(context.__RESULT).length, 101);
  assert.equal(context.compileCount, 1);
});

test("live role output does not linearly search all elements for every hit", () => {
  const { context } = fixture();
  vm.runInContext(
    `
    var indexSearches = 0;
    var originalIndexOf = Array.prototype.indexOf;
    Array.prototype.indexOf = function (...args) { indexSearches++; return originalIndexOf.apply(this, args); };
    __G.tcGetBy('role', 'button');
  `,
    context,
  );
  const matches = JSON.parse(context.__RESULT);
  assert.equal(matches.length, 101);
  assert.deepEqual(
    matches.map((hit) => hit.idx),
    Array.from({ length: 101 }, (_, i) => i + 1),
  );
  assert.equal(context.indexSearches, 0);
});

function copyMetadataFixture(node) {
  return { children: node.children.map(copyMetadataFixture) };
}

test("inert locator copies cross the native mutation boundary once per subtree", () => {
  const source = readFileSync(
    new URL("../crates/turbo-surf-render/src/locator_dom.js", import.meta.url),
    "utf8",
  );
  const copyStart = source.indexOf("function __locatorInertCopy(");
  const copyEnd = source.indexOf("\nfunction __locatorMetadataRoot", copyStart);
  let imports = 0;
  let appends = 0;
  const root = { children: Array.from({ length: 1000 }, () => ({ children: [] })) };
  const context = vm.createContext({
    __locatorMetadataOwner: () => ({}),
    __locatorApply: Reflect.apply,
    __locatorImport(node, deep) {
      imports++;
      return deep ? copyMetadataFixture(node) : { children: [] };
    },
    __locatorAppend(child) {
      appends++;
      this.children.push(child);
    },
    __locatorChildren: (node) => node.children,
    __list: () => [],
    __locatorPush: (list, value) => list.push(value),
    __locatorPop: (list) => list.pop(),
    root,
  });
  vm.runInContext(
    `${source.slice(copyStart, copyEnd)}; result = __locatorInertCopy(root);`,
    context,
  );
  assert.deepEqual(context.result, copyMetadataFixture(root));
  assert.equal(imports, 1, "one native import must copy the whole subtree");
  assert.equal(appends, 0, "copying must not reconcile native ownership per child");
});
