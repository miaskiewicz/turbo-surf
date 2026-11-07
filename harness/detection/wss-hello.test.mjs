// Offline checks for the identity battery's wss: TLS row (wss-hello.mjs, turbo-surf#179)
// over the recorded Chrome 156 control. No browser, no network.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { describeClientHello } from "./identity-wire.mjs";
import { diffWss, helloFacts, helloView } from "./wss-hello.mjs";

const here = (name) => fileURLToPath(new URL(name, import.meta.url));
const control = JSON.parse(readFileSync(here("fixtures/wss-hello-chrome156.json"), "utf8"));
const knownGaps = JSON.parse(readFileSync(here("chrome-identity-known-gaps.json"), "utf8"));
const clone = (value) => JSON.parse(JSON.stringify(value));
const tells = (result) => result.deltas.filter((delta) => delta.class === "tell");

test("the wss control was recorded by this collector from its raw records", () => {
  const collector = createHash("sha256")
    .update(readFileSync(here("wss-hello.mjs")))
    .digest("hex");
  assert.equal(control.collector, collector, "re-record with wss-hello.mjs --chrome");
  assert.match(control.browser.product, /^HeadlessChrome\/156\./);
  for (const mode of ["https", "wss"])
    assert.deepEqual(
      control.views[mode],
      helloView(describeClientHello(Buffer.from(control.raw[mode], "base64"))),
    );
});

test("Chrome's wss hello is its https hello with ALPN http/1.1 only and no ALPS", () => {
  const https = helloFacts(control.views.https);
  const wss = helloFacts(control.views.wss);
  const changed = Object.keys({ ...https, ...wss })
    .filter((key) => JSON.stringify(https[key]) !== JSON.stringify(wss[key]))
    .sort();
  assert.deepEqual(changed, [
    "alpn",
    "applicationSettings",
    "extension.16",
    "extension.17613",
    "ja4.alpn",
  ]);
  assert.deepEqual(wss.alpn, ["http/1.1"]);
  assert.equal(wss.applicationSettings, null);
  assert.equal(wss["ja4.alpn"], "h1");
});

test("a native side matching the control has no tells", () => {
  const result = diffWss(control, { views: clone(control.views) }, { knownGaps });
  assert.deepEqual(result.deltas, []);
});

test("an oracle https difference shared by wss is not a tell", () => {
  const native = { views: clone(control.views) };
  for (const mode of ["https", "wss"]) native.views[mode].trustAnchors = "00ff";
  const result = diffWss(control, native, { knownGaps });
  assert.deepEqual(tells(result), []);
  assert.deepEqual(result.summary, { "oracle-https": 1 });
});

test("a wss hello that keeps the https ALPN or ALPS is a tell", () => {
  const native = { views: clone(control.views) };
  native.views.wss = clone(control.views.https);
  const result = diffWss(control, native, { knownGaps });
  const paths = new Set(tells(result).map((delta) => delta.path));
  for (const path of ["alpn", "applicationSettings", "extension.17613", "ja4.alpn"])
    assert.ok(paths.has(path), path);
  assert.ok(tells(result).some((delta) => delta.type === "ws-delta"));
  assert.ok(result.deltas.every((delta) => delta.area === "wss" && delta.origin === "W"));
});

test("a wss hello from another TLS stack (rustls ciphers) is a tell", () => {
  const native = { views: clone(control.views) };
  native.views.wss.hello.ciphers = [4866, 4865, 4867, 49196, 49195, 52393, 49200, 49199, 52392];
  const result = diffWss(control, native, { knownGaps });
  assert.deepEqual(
    tells(result).map((delta) => [delta.type, delta.path]),
    [
      ["ws-delta", "ciphers"],
      ["value", "ciphers"],
    ],
  );
});
