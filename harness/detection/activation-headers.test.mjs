import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

test("activation header Chrome wire fixture is source pinned and covers real navigation chains", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/activation-headers-chrome155.json", import.meta.url), "utf8"),
  );
  const source = readFileSync(new URL("./activation-headers-chrome.mjs", import.meta.url));
  assert.equal(fixture.sourceSha256, createHash("sha256").update(source).digest("hex"));
  assert.match(fixture.browser, /^Chrome\/155\./);
  assert.equal(fixture.freshProfile, true);
  assert.ok(fixture.realElapsedExpiryMs > 5000);
  assert.deepEqual(fixture.errors, []);
  assert.equal(fixture.results.length, 14);
  for (const control of fixture.results) {
    assert.equal(control.requests[0].phase, control.name.includes("-location") ? "next" : "submit");
    assert.equal(control.requests[1].phase, "destination");
    assert.equal(control.requests[0].secFetchUser, control.requests[1].secFetchUser);
    assert.ok(control.requests.every((row) => row.name === control.name && row.method === "GET"));
    assert.ok(control.requests.slice(2).every((row) => row.secFetchUser === null));
  }
  assert.deepEqual(
    fixture.results.find((row) => row.name === "automatic-retry").requests.map((row) => row.phase),
    ["submit", "destination", "retry", "final"],
  );
});

test("activation oracle distinguishes cross-origin sticky state from redirect request activation", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/activation-headers-chrome155.json", import.meta.url), "utf8"),
  );
  const controls = new Map(fixture.results.map((row) => [row.name, row]));
  assert.deepEqual(controls.get("trusted-click").destinationActivation, {
    isActive: false,
    hasBeenActive: true,
  });
  for (const name of ["trusted-cross-origin-form", "trusted-cross-origin-redirect"]) {
    const control = controls.get(name);
    assert.deepEqual(control.destinationActivation, { isActive: false, hasBeenActive: false });
    assert.equal(control.requests[1].origin, "secondary");
    assert.equal(control.requests[1].secFetchUser, "?1");
  }
  assert.equal(controls.get("trusted-cross-origin-form").requests[0].origin, "secondary");
  assert.equal(controls.get("trusted-cross-origin-redirect").requests[0].origin, "primary");
});
