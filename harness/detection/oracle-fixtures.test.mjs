import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("saved Chrome controls retain the exact collector source they measured", () => {
  for (const [name, collector] of [
    ["classic-script-errors", "classic-script-errors"],
    ["document-legacy", "document-legacy"],
    ["event-ownership", "event-ownership"],
    ["popover-lifecycle", "popover-lifecycle"],
    ["message-transfers", "message-transfer"],
    ["range-live", "range-live"],
    ["dom-adoption", "dom-adoption"],
    ["dom-variadic-errors", "dom-variadic-errors"],
    ["frame-insertion-errors", "frame-insertion-errors"],
    ["mutation-events", "mutation-events"],
  ]) {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/${name}-chrome155.json`, import.meta.url), "utf8"),
    );
    const source = readFileSync(new URL(`./${collector}-probe.js`, import.meta.url));
    assert.equal(
      createHash("sha256").update(source).digest("hex"),
      fixture.collectorSha256,
      `${name}: remeasure the Chrome control after changing its collector`,
    );
    assert.match(fixture.browserVersion, /^Chrome\/155\./);
    assert.ok(fixture.result && typeof fixture.result === "object");
  }
});

test("iframe ownership controls retain their exact measured collectors", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/frame-ownership-chrome155.json", import.meta.url), "utf8"),
  );
  assert.deepEqual(Object.keys(fixture.collectors).sort(), [
    "frame-adoption-probe.js",
    "frame-native-base-probe.js",
    "frame-ownership-probe.js",
  ]);
  for (const [collector, expected] of Object.entries(fixture.collectors)) {
    const source = readFileSync(new URL(`./${collector}`, import.meta.url));
    assert.equal(createHash("sha256").update(source).digest("hex"), expected, collector);
  }
  assert.match(fixture.browserVersion, /^Chrome\/155\./);
  assert.ok(fixture.ownership && fixture.adoption && fixture.baseResolution);
});

test("native microtask and module controls retain their measured probe and Chrome collector", () => {
  const collector = readFileSync(new URL("./authored-dom-chrome.mjs", import.meta.url));
  for (const name of ["microtask-order", "microtask-frame", "dom-utf16-module"]) {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/${name}-chrome155.json`, import.meta.url), "utf8"),
    );
    const probe = readFileSync(new URL(`./${name}-probe.js`, import.meta.url));
    assert.equal(createHash("sha256").update(probe).digest("hex"), fixture.probeSha256, name);
    assert.equal(createHash("sha256").update(collector).digest("hex"), fixture.collectorSha256);
    assert.deepEqual(fixture.errors, []);
    assert.equal(fixture.browser, "Chrome/155.0.8059.79");
    assert.equal(fixture.v8, "15.5.35.23");
  }
});
