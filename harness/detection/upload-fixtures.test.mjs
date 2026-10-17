import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (name) => readFileSync(new URL(name, import.meta.url));
const hash = (name) => createHash("sha256").update(read(name)).digest("hex");

test("HTTPS upload oracle pins normal Chrome, source and fixture-only TLS trust", () => {
  const fixture = JSON.parse(read("fixtures/upload-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.80");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("upload-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-upload-chrome.mjs"));
  assert.equal(fixture.fixtureSha256, hash("upload-server.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.equal(Object.keys(fixture.result).length, 9);
  assert.equal(fixture.flags.includes("--ignore-certificate-errors"), false);
  assert.equal(
    fixture.flags.some((flag) => flag.startsWith("--ignore-certificate-errors-spki-list=")),
    true,
  );
  assert.equal(
    fixture.wire.some((request) => request.id === "h1" && request.method === "POST"),
    false,
  );
  assert.equal(fixture.result.sameOrigin.beforeClose, true);
  assert.equal(fixture.result.crossOrigin.result.options, 1);
  assert.equal(fixture.result.backpressure.bounded, true);
  assert.equal(fixture.result.earlyResponse.beforeClose, false);
});

test("stalled-peer abort oracle pins immediate rejection and reason identity", () => {
  const fixture = JSON.parse(read("fixtures/upload-abort-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.80");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("upload-abort-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-upload-chrome.mjs"));
  assert.equal(fixture.fixtureSha256, hash("upload-server.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.deepEqual(fixture.result, {
    rejectedWithReason: true,
    rejectedWithoutWaitingForCleanup: true,
    sourceReasonIdentity: true,
  });
});

test("buffered HTTP2 response oracle pins genuine Chrome and executed source", () => {
  const fixture = JSON.parse(read("fixtures/response-h2-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.80");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("response-h2-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-upload-chrome.mjs"));
  assert.equal(fixture.fixtureSha256, hash("upload-server.mjs"));
  assert.deepEqual(fixture.errors, []);
});
