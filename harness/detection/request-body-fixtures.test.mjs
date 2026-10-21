import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (name) => readFileSync(new URL(name, import.meta.url));
const hash = (name) => createHash("sha256").update(read(name)).digest("hex");

test("request BodyInit oracle pins the executed source and genuine Chrome version", () => {
  const fixture = JSON.parse(read("fixtures/request-body-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.79");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("request-body-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-request-body-chrome.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.equal(Object.keys(fixture.result).length, 12);
  assert.equal(
    fixture.wire.some((request) => request.path === "/echo"),
    false,
  );
});

test("adversarial BodyInit oracle pins private brands, transfer and byte chunk rejection", () => {
  const fixture = JSON.parse(read("fixtures/request-body-details-chrome155.json"));
  assert.equal(fixture.browser, "Chrome/155.0.8059.79");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.probeSha256, hash("request-body-details-probe.js"));
  assert.equal(fixture.collectorSha256, hash("authored-request-body-chrome.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.equal(Object.keys(fixture.result).length, 16);
  assert.equal(fixture.wire.filter((request) => request.path === "/echo").length, 1);
  assert.deepEqual(fixture.result.invalidChunks, Array(4).fill("TypeError"));
});
