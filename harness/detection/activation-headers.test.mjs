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

test(
  "keyboard-driven activation controls always launch headless, away from the OS keyboard focus",
  { todo: "turbo-surf#52" },
  () => {
    // A headed Chrome takes the OS keyboard focus on launch, so host keystrokes would
    // land next to the collector's trusted Enter/Space/Escape input (turbo-surf#52).
    for (const [collector, control] of [
      ["./activation-headers-chrome.mjs", "./fixtures/activation-headers-chrome156.json"],
      ["./user-activation-gates-chrome.mjs", "./fixtures/user_activation_gates_chrome156.json"],
    ]) {
      const source = readFileSync(new URL(collector, import.meta.url), "utf8");
      assert.doesNotMatch(source, /TURBO_SURF_CHROME_HEADLESS/, collector);
      const fixture = JSON.parse(readFileSync(new URL(control, import.meta.url), "utf8"));
      assert.match(fixture.browser, /^Chrome\/156\./, control);
      assert.equal(fixture.headless, true, control);
      assert.equal(fixture.flags.filter((flag) => flag === "--headless=new").length, 1, control);
      assert.equal(fixture.flags.at(-1), "about:blank", control);
      assert.equal(fixture.flags.at(-2), "--headless=new", control);
    }
  },
);
