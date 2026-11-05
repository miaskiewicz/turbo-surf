import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("saved Chrome controls retain the exact collector source they measured", () => {
  // dom-adoption and dom-variadic-errors were re-recorded on the frozen Chrome 156 headless
  // shell after the #154 refactor; the rest remain Chrome 155 controls.
  const chrome155 = /^Chrome\/155\./;
  const chrome156 = /^HeadlessChrome\/156\.0\.8078\.47$/;
  for (const [name, collector, browser] of [
    ["classic-script-errors", "classic-script-errors", chrome155],
    ["document-legacy", "document-legacy", chrome155],
    ["event-ownership", "event-ownership", chrome155],
    ["popover-lifecycle", "popover-lifecycle", chrome155],
    ["message-transfers", "message-transfer", chrome155],
    ["range-live", "range-live", chrome155],
    ["dom-adoption", "dom-adoption", chrome156],
    ["dom-variadic-errors", "dom-variadic-errors", chrome156],
    ["frame-insertion-errors", "frame-insertion-errors", chrome155],
    ["mutation-events", "mutation-events", chrome155],
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
    assert.match(fixture.browserVersion, browser);
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
  for (const name of [
    "microtask-order",
    "microtask-frame",
    "dom-utf16-module",
    "frame-load-order",
    "security-policy-event",
    "module-inline-load",
  ]) {
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

test("CSP violation controls pin their original collectors, cases and probes", () => {
  for (const [name, collectorName, probeName = name] of [
    ["csp-violation", "policy-header-chrome.mjs"],
    ["csp-parser", "csp-document-chrome.mjs"],
    ["parser-source-position", "csp-document-chrome.mjs"],
    ["csp-resource", "csp-resource-chrome.mjs"],
    ["csp-code-generation", "csp-resource-chrome.mjs"],
    ["csp-navigation", "csp-navigation-chrome.mjs"],
    ["csp-referrer-meta", "csp-referrer-meta-chrome.mjs", "csp-navigation"],
  ]) {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/${name}-chrome155.json`, import.meta.url), "utf8"),
    );
    for (const [file, expected] of [
      [`./${probeName}-probe.js`, fixture.probeSha256],
      [`./fixtures/${name}-cases.json`, fixture.casesSha256],
      [`./${collectorName}`, fixture.collectorSha256],
    ]) {
      assert.equal(
        createHash("sha256")
          .update(readFileSync(new URL(file, import.meta.url)))
          .digest("hex"),
        expected,
        file,
      );
    }
    assert.deepEqual(fixture.errors, []);
    assert.equal(fixture.browser, "Chrome/155.0.8059.79");
    assert.equal(fixture.v8, "15.5.35.23");
  }
});
