"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const { checkSummaries, checkSkips, skipRows } = require("./js-test-completeness.cjs");

const summary = (extra = "", tests = 1, passed = 1, skipped = 0) =>
  `TAP version 13\nok 1 - actual behavior\n${extra}# tests ${tests}\n# suites 0\n# pass ${passed}\n# fail 0\n# cancelled 0\n# skipped ${skipped}\n# todo 0\n`;

test("all command summaries must be present, nonempty and complete", () => {
  assert.equal(checkSummaries(summary(), 1).length, 1);
  for (const text of [
    "",
    summary("", 0, 0),
    summary().replace("# fail 0", "# fail 1"),
    summary().replace("# cancelled 0", "# cancelled 1"),
    summary().replace("# todo 0", "# todo 1"),
    summary().replace("# pass 1", "# pass 0"),
  ]) {
    assert.throws(() => checkSummaries(text, 1));
  }
  assert.throws(() => checkSummaries(summary(), 3), /Missing/);
  assert.throws(
    () => checkSummaries(summary("ok 2 - /fixture/empty.test.mjs\n"), 1),
    /without registering/,
  );
});

test("exact three intentional skips are allowed and never counted as passing cases", () => {
  const text = summary(
    "ok 2 - skipped test does not run # SKIP\nok 3 - ordinary must not run # SKIP excluded by focused tests\nok 4 - ordinary must not run # SKIP excluded by focused tests\n",
    4,
    1,
    3,
  );
  assert.doesNotThrow(() => checkSummaries(text, 1));
  assert.doesNotThrow(() => checkSkips(skipRows(text)));
  assert.throws(() => checkSkips([]), /inventory changed/);
  assert.throws(
    () => checkSkips([...skipRows(text), { name: "required trace missing", reason: "" }]),
    /Unexpected/,
  );
  assert.throws(() => checkSkips([...skipRows(text), skipRows(text)[0]]), /Unexpected/);
  assert.throws(
    () => checkSkips(skipRows(text.replace("excluded by focused tests", "addon missing"))),
    /Unexpected/,
  );
});

test("exit-zero optional-input skips cannot masquerade as a complete test run", () => {
  const text = summary("ok 2 - native trace required # SKIP missing binary\n", 2, 1, 1);
  assert.doesNotThrow(() => checkSummaries(text, 1));
  assert.throws(() => checkSkips(skipRows(text)), /Unexpected skipped test: native trace required/);
  assert.throws(() => checkSummaries(summary("", 2, 1, 1), 1), /accounting/);
});
