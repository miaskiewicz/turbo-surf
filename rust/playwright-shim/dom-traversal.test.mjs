import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import test from "node:test";

function node(id, ...children) {
  const value = {
    id,
    nodeType: 1,
    nodeName: id === "section" ? "SECTION" : "SPAN",
    parentNode: null,
  };
  value.firstChild = children[0] ?? null;
  value.lastChild = children.at(-1) ?? null;
  children.forEach((child, i) => {
    child.parentNode = value;
    child.previousSibling = children[i - 1] ?? null;
    child.nextSibling = children[i + 1] ?? null;
  });
  return value;
}
function document() {
  const runtime = readFileSync(
    new URL("../crates/turbo-surf-render/src/runtime.rs", import.meta.url),
    "utf8",
  );
  const start = runtime.indexOf("// document.createTreeWalker + NodeFilter");
  const source =
    start < 0
      ? readFileSync(
          new URL("../crates/turbo-surf-render/src/dom_traversal.js", import.meta.url),
          "utf8",
        )
      : runtime.slice(start, runtime.indexOf("// Viewport / screen globals", start));
  const context = vm.createContext({ document: {} });
  vm.runInContext(source, context);
  return context.document;
}

test("TreeWalker traverses backward and includes its accepted root as a parent", () => {
  const root = node("root", node("a"), node("b"));
  const walker = document().createTreeWalker(root, 1);
  assert.equal(walker.nextNode().id, "a");
  assert.equal(walker.nextNode().id, "b");
  assert.equal(walker.previousNode().id, "a");
  assert.equal(walker.parentNode(), root);
  assert.equal(walker.previousNode(), null);
});

test("TreeWalker child and sibling traversal descends skipped but prunes rejected subtrees", () => {
  const inside = node("inside");
  const root = node(
    "root",
    node("section", inside),
    node("reject", node("hidden")),
    node("sibling"),
  );
  const walker = document().createTreeWalker(root, 1, (n) =>
    n.id === "section" ? 3 : n.id === "reject" ? 2 : 1,
  );
  assert.equal(walker.firstChild(), inside);
  assert.equal(walker.nextSibling().id, "sibling");
  assert.equal(walker.previousSibling(), inside);
  assert.equal(walker.parentNode(), root);
  assert.equal(walker.lastChild().id, "sibling");
  assert.equal(walker.previousNode(), inside);
});

test("NodeIterator includes its root and reverses around its reference pointer", () => {
  const root = node("root", node("child"));
  const iterator = document().createNodeIterator(root, 1);
  assert.equal(iterator.referenceNode, root);
  assert.equal(iterator.pointerBeforeReferenceNode, true);
  assert.equal(iterator.nextNode(), root);
  assert.equal(iterator.pointerBeforeReferenceNode, false);
  assert.equal(iterator.nextNode().id, "child");
  assert.equal(iterator.nextNode(), null);
  assert.equal(iterator.previousNode().id, "child");
  assert.equal(iterator.previousNode(), root);
  assert.equal(iterator.previousNode(), null);
});

test("NodeIterator rejection skips a node without pruning its descendants", () => {
  const child = node("child");
  const root = node("root", node("reject", child));
  const iterator = document().createNodeIterator(root, 1, (n) => (n.id === "reject" ? 2 : 1));
  assert.equal(iterator.nextNode(), root);
  assert.equal(iterator.nextNode(), child);
  assert.equal(iterator.previousNode(), child);
  assert.equal(iterator.previousNode(), root);
});
