import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (name) => readFileSync(new URL(name, import.meta.url));
const hash = (name) => createHash("sha256").update(read(name)).digest("hex");

test("stream-transfer control records genuine Chrome behavior and executable sources", () => {
  const fixture = JSON.parse(read("fixtures/stream-transfer-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.80");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("stream-transfer-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-upload-chrome.mjs"));
  assert.equal(fixture.fixtureSha256, hash("upload-server.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.equal(Object.keys(fixture.result).length, 20);
  assert.deepEqual(fixture.result.failedGraph, {
    error: "DataCloneError",
    bufferBytes: 8,
    locked: false,
  });
  assert.deepEqual(fixture.result.lateLock, {
    error: "DataCloneError",
    bufferBytes: 8,
    locked: true,
  });
  assert.deepEqual(fixture.result.writable.accepted, { value: 7 });
  assert.equal(fixture.result.readable.differentChunk, true);
  assert.equal(fixture.result.messagePort.ports, 0);
  assert.equal(fixture.result.window.ports, 0);
});
