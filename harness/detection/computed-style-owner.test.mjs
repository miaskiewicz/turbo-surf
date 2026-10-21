import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
let native;
try {
  native = require("../../rust/crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
}
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const hash = (path) => createHash("sha256").update(read(path)).digest("hex");
const cases = JSON.parse(read("./fixtures/computed-style-owner-cases.json"));
const probe = read("./computed-style-owner-probe.js");
const control = JSON.parse(read("./fixtures/computed-style-owner-chrome155.json"));

test("computed-style foreign owner control records independently captured Chrome source hashes", () => {
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.casesSha256, hash("./fixtures/computed-style-owner-cases.json"));
  assert.equal(control.probeSha256, hash("./computed-style-owner-probe.js"));
  assert.equal(control.collectorSha256, hash("./trusted-types-code-chrome.mjs"));
  assert.deepEqual(control.errors, []);
});

test(
  "parent computed-style reads retain the real child owner and refresh across foreign CSSOM edits",
  { skip: !native && "native addon is not built" },
  () => {
    const actual = JSON.parse(
      native.evaluate(cases[0].html, `JSON.stringify(${probe.trim().replace(/;$/, "")})`),
    );
    assert.deepEqual(actual, control.result[0].result);
  },
);
