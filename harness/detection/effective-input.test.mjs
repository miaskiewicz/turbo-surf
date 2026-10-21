import assert from "node:assert/strict";
import { test } from "node:test";
import {
  chromeExecutable,
  effectiveTypedControl,
  prepareTypedControl,
  requireEffectiveInput,
} from "./effective-input.mjs";
const control = (value = "fixture query") => {
  const node = { tagName: "INPUT", value, isConnected: true };
  node.ownerDocument = { activeElement: node };
  return node;
};

test("effective query guard requires exact text and retained focused connected node", () => {
  const node = control();
  assert.equal(requireEffectiveInput(effectiveTypedControl(node, "fixture query")).focused, true);
  node.value = "fixture qu";
  assert.throws(
    () => requireEffectiveInput(effectiveTypedControl(node, "fixture query")),
    /Enter was not sent/,
  );
  node.value = "fixture query";
  node.ownerDocument.activeElement = control();
  assert.throws(() => requireEffectiveInput(effectiveTypedControl(node, "fixture query")));
  node.ownerDocument.activeElement = node;
  node.isConnected = false;
  assert.throws(() => requireEffectiveInput(effectiveTypedControl(node, "fixture query")));
});

test("expectations preserve append, selection replacement and successive chunks", () => {
  const node = control("prefilled ");
  let plan = prepareTypedControl(node, "first");
  assert.equal(plan.expected, "prefilled first");
  node.value = plan.expected;
  plan = prepareTypedControl(node, " second");
  assert.equal(plan.expected, "prefilled first second");
  node.selectionStart = 0;
  node.selectionEnd = 9;
  assert.equal(prepareTypedControl(node, "new").expected, "new first");
  node.ownerDocument.activeElement = { tagName: "BODY" };
  assert.throws(() => prepareTypedControl(node, "ignored"), /remain focused/);
  assert.equal(node.value, "prefilled first");
});

test("same-selector replacement does not replace the retained identity", () => {
  const original = control("query"),
    replacement = control("query");
  original.ownerDocument.activeElement = replacement;
  original.isConnected = false;
  assert.throws(() => requireEffectiveInput(effectiveTypedControl(original, "query")));
});

test("explicit Chrome executable wins over environment then installed channel", () => {
  assert.equal(
    chromeExecutable({ chromePath: "/explicit" }, { TURBO_SURF_CHROME_PATH: "/env" }),
    "/explicit",
  );
  assert.equal(chromeExecutable({}, { TURBO_SURF_CHROME_PATH: "/env" }), "/env");
  assert.equal(chromeExecutable({}, {}), null);
});
