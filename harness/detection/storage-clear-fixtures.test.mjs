import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

test("storage-clear control pins actual Chrome, source, collector and widget", () => {
  const control = JSON.parse(
    readFileSync(new URL("fixtures/storage-clear-chrome155.json", import.meta.url)),
  );
  const sha = (path) =>
    createHash("sha256")
      .update(readFileSync(new URL(path, import.meta.url)))
      .digest("hex");
  assert.equal(control.browser, "Chrome/155.0.8059.79");
  assert.equal(control.v8, "15.5.35.23");
  assert.equal(control.probeSha256, sha("storage-clear-probe.js"));
  assert.equal(control.collectorSha256, sha("authored-storage-clear-chrome.mjs"));
  assert.equal(control.widgetSha256, sha("storage-clear-widget.js"));
  assert.deepEqual(control.errors, []);
  assert.deepEqual(control.result.iframeCrossSite, { local: null, session: null });
  assert.deepEqual(control.result.iframeSameSite, { local: null, session: null });
  for (const name of ["sameSiteDenied", "sameSiteCorpDenied"]) {
    assert.equal(control.result[name].status, "TypeError");
    assert.deepEqual(control.result[name].child, { local: null, session: null });
  }
  assert.deepEqual(control.result.sameSiteOmit.child, { local: "child", session: "child" });
  assert.deepEqual(control.result.empty.clearEvents.local, []);
  assert.equal(control.result.empty.clearEvents.session.length, 1);
  assert.equal(control.wire.filter((request) => request.case === "replay").length, 1);
});
