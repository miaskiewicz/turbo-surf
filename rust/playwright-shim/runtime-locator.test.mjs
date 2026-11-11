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
  const context = vm.createContext({ document, __G: {} });
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
