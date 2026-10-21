import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("./input-state.js", import.meta.url), "utf8");
const captureInputState = runInNewContext(source);
const hidden = () => ({
  tagName: "INPUT",
  type: "hidden",
  get value() {
    throw new Error("Hidden challenge payload must not be read");
  },
});
const query = (tagName = "INPUT") => ({
  tagName,
  type: tagName === "INPUT" ? "text" : "textarea",
  value: "rust lang",
  disabled: false,
  readOnly: false,
});
function snapshot(candidates, consent = false) {
  const document = {
    activeElement: { tagName: "BODY", getAttribute: () => null },
    querySelectorAll(selector) {
      assert.equal(selector, 'textarea[name="q"],input[name="q"]');
      return candidates;
    },
    querySelector(selector) {
      assert.equal(selector, "#consent-fixture");
      return consent ? {} : null;
    },
  };
  return JSON.parse(JSON.stringify(captureInputState(document, "#consent-fixture")));
}

test("query snapshots skip hidden q before and after real controls without reading hidden values", () => {
  for (const tag of ["INPUT", "TEXTAREA"])
    for (const candidates of [
      [hidden(), query(tag)],
      [query(tag), hidden()],
    ]) {
      const state = snapshot(candidates, true);
      assert.equal(state.selection, "first-q-control-with-non-hidden-type");
      assert.equal(state.queryControlCount, 2);
      assert.equal(state.excludedHiddenQueryControls, 1);
      assert.equal(state.controlTag, tag);
      assert.equal(state.controlType, tag === "INPUT" ? "text" : "textarea");
      assert.equal(state.value, "rust lang");
      assert.equal(state.consentPresent, true);
    }
});

test("challenge-only and absent controls have null values and explicit type-filter counts", () => {
  for (const candidates of [[], [hidden()], [hidden(), hidden()]]) {
    const state = snapshot(candidates);
    assert.equal(state.queryControlCount, candidates.length);
    assert.equal(state.excludedHiddenQueryControls, candidates.length);
    for (const key of ["controlTag", "controlType", "value", "disabled", "readOnly"])
      assert.equal(state[key], null);
    assert.equal(state.consentPresent, false);
  }
});
