// Offline tests for the Chrome identity battery, its diff engine and the
// re-version helper. Expected versions come from the registry and the default
// fixture, never from literals, so a Chrome bump only regenerates data.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  ACCEPT_CH,
  batteryRoute,
  collectorHashes,
  labelize,
  normalizeRequest,
} from "./chrome-identity-battery.mjs";
import { importLegacy } from "./chrome-identity-import.mjs";
import {
  NOJS_KINDS,
  RENDER_KINDS,
  brandHeader,
  brandVersions,
  diffParity,
  diffVersions,
  matches,
  measuredRelease,
  mergeOrders,
  normalizeBrands,
  normalizeVersion,
  requestClass,
  versionOf,
  withCookieSlot,
} from "./identity-diff.mjs";
import { H2FrameTap, describeClientHello, fingerprints, summarizeH2 } from "./identity-wire.mjs";
import {
  applyRelease,
  blockers,
  compareVersions,
  currentIdentity,
  drift,
  engineStep,
  readRegistry,
  releaseStatus,
} from "./reversion-chrome.mjs";

const read = (path) => JSON.parse(readFileSync(new URL(path, import.meta.url)));
const registry = readRegistry();
const { release: current, fixture } = currentIdentity(registry);
const sha256 = (data) => createHash("sha256").update(data).digest("hex");
const clone = (value) => structuredClone(value);

test("the default registry release has a complete fixture measured by this collector", () => {
  assert.ok(fixture, `fixtures/chrome-identity-${registry.defaultMajor}.json must exist`);
  assert.equal(fixture.partial, undefined, "the default release needs a full battery capture");
  assert.deepEqual(fixture.missing, []);
  assert.deepEqual(fixture.errors, []);
  assert.deepEqual(fixture.collector, collectorHashes(), "re-run reversion-chrome.mjs");
  assert.equal(versionOf(fixture).full, current.fullVersion);
  assert.equal(fixture.browser.product, `Chrome/${current.fullVersion}`);
  assert.equal(
    current.evidence.fixture,
    `harness/detection/fixtures/chrome-identity-${current.major}.json`,
  );
});

test("the registry entry is exactly what the default fixture measures", () => {
  const previous = registry.releases.find((entry) => entry.major < current.major);
  const { evidence, ...stored } = current;
  assert.deepEqual(measuredRelease(fixture, previous), stored);
  assert.equal(evidence.v8, fixture.browser.v8);
});

test("the GREASE brand algorithm reproduces every measured Chrome release", () => {
  for (const name of ["fixtures/chrome-identity-155.json", current.evidence.fixture.slice(18)]) {
    const measured = read(name);
    const { major, full } = versionOf(measured);
    assert.deepEqual(measured.js.userAgentData.brands, brandVersions(major, String(major), false));
    assert.deepEqual(
      measured.js.userAgentData.high.fullVersionList,
      brandVersions(major, full, true),
    );
    const nav = measured.requests.find((request) => request.kind === "nav");
    const header = nav.headers.find(([name]) => name === "sec-ch-ua")[1];
    assert.equal(header, brandHeader(major, String(major), false));
  }
});

test("version normalization removes every spelling of the release", () => {
  const version = { major: 156, full: "156.0.8078.47" };
  const ua = "Mozilla/5.0 (Macintosh) Chrome/156.0.0.0 Safari/537.36";
  assert.equal(
    normalizeVersion(ua, version),
    "Mozilla/5.0 (Macintosh) Chrome/{MAJOR}.0.0.0 Safari/537.36",
  );
  assert.equal(normalizeVersion(brandHeader(156, "156", false), version), "{BRANDS}");
  assert.equal(normalizeVersion(brandHeader(156, version.full, true), version), "{FULL_BRANDS}");
  assert.deepEqual(normalizeBrands({ b: brandVersions(156, "156", false) }, version), {
    b: "{BRANDS}",
  });
  assert.deepEqual(normalizeBrands({ b: brandVersions(155, "155", false) }, version).b.length, 3);
});

test("request classes mirror the Rust classifier", () => {
  const h = (pairs) => Object.entries(pairs);
  assert.equal(requestClass(h({ "sec-fetch-mode": "navigate" }), "GET"), "navigate");
  assert.equal(requestClass(h({ "Upgrade-Insecure-Requests": "1" }), "GET"), "navigate");
  assert.equal(requestClass(h({ "sec-fetch-mode": "no-cors" }), "GET"), "no-cors");
  assert.equal(
    requestClass(h({ "sec-fetch-mode": "cors", "sec-fetch-dest": "font" }), "GET"),
    "cors-resource",
  );
  assert.equal(
    requestClass(h({ "sec-fetch-mode": "cors", "sec-fetch-dest": "empty" }), "PUT"),
    "cors",
  );
  assert.equal(
    requestClass(
      h({ "sec-fetch-mode": "cors", "access-control-request-method": "PUT" }),
      "OPTIONS",
    ),
    "preflight",
  );
  assert.equal(
    requestClass(h({ "sec-fetch-mode": "cors", "sec-ch-ua-arch": '"arm"' }), "GET"),
    "cors-hinted",
  );
});

test("order merging keeps every sample's order and reports contradictions", () => {
  assert.deepEqual(
    mergeOrders([
      ["a", "c"],
      ["a", "b", "c"],
      ["c", "d"],
    ]).order,
    ["a", "b", "c", "d"],
  );
  assert.deepEqual(
    mergeOrders([
      ["a", "b"],
      ["b", "a"],
    ]).conflict,
    ["a", "b"],
  );
  assert.deepEqual(
    withCookieSlot(["x", "accept-language", "priority"], ["accept-language", "cookie", "priority"]),
    ["x", "accept-language", "cookie", "priority"],
  );
  assert.deepEqual(withCookieSlot(["a", "cookie"], []), ["a", "cookie"]);
  assert.deepEqual(withCookieSlot(["a"], []), ["a", "cookie"]);
});

test("ClientHello parsing reproduces the stored JA4 and stays GREASE-insensitive", () => {
  const chrome = read("TRANSPORT-156.json".replace("156", String(current.major)));
  const bytes = readFileSync(new URL(chrome.rawRecords.chrome.fixture, import.meta.url));
  assert.equal(sha256(bytes), chrome.rawRecords.chrome.sha256);
  const hello = describeClientHello(bytes);
  assert.equal(hello.ja4, chrome.fingerprints.chrome.ja4);
  assert.match(hello.ja4, /^t13[di]\d{4}h2_[0-9a-f]{12}_[0-9a-f]{12}$/);
  assert.ok(hello.groupNames.includes("X25519MLKEM768"));
  assert.deepEqual(hello.applicationSettings, { codepoint: 17613, protocols: ["h2"] });
  // JA3N and JA4 sort extensions, so Chrome's per-connection permutation cancels out.
  const shuffled = { ...hello, extensions: [...hello.extensions].reverse() };
  const fp = fingerprints(shuffled, { pointFormats: hello.pointFormats });
  assert.equal(fp.ja4, hello.ja4);
  assert.equal(fp.ja3nHash, hello.ja3nHash);
  assert.notEqual(fp.ja3Hash, hello.ja3Hash);
  assert.throws(() => describeClientHello(bytes.subarray(0, 40)), /truncated|invalid/);
});

test("the H2 frame tap decodes Chrome's preamble across arbitrary chunking", () => {
  const frame = (type, flags, stream, payload) => {
    const header = Buffer.alloc(9);
    header.writeUIntBE(payload.length, 0, 3);
    header[3] = type;
    header[4] = flags;
    header.writeUInt32BE(stream, 5);
    return Buffer.concat([header, payload]);
  };
  const settings = Buffer.alloc(24);
  [
    [1, 65536],
    [2, 0],
    [4, 6291456],
    [6, 262144],
  ].forEach(([id, value], i) => {
    settings.writeUInt16BE(id, i * 6);
    settings.writeUInt32BE(value, i * 6 + 2);
  });
  const update = Buffer.alloc(4);
  update.writeUInt32BE(15663105);
  const headers = Buffer.concat([Buffer.from([0x80, 0, 0, 0, 255]), Buffer.from([0x82])]);
  const wire = Buffer.concat([
    Buffer.from("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"),
    frame(4, 0, 0, settings),
    frame(8, 0, 0, update),
    frame(1, 0x25, 1, headers),
  ]);
  const tap = new H2FrameTap();
  for (let i = 0; i < wire.length; i += 7) tap.push(wire.subarray(i, i + 7));
  const summary = summarizeH2(tap.frames, [":method", ":authority", ":scheme", ":path"]);
  assert.equal(summary.akamai, "1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p");
  assert.deepEqual(summary.firstHeadersPriority, { exclusive: true, dependency: 0, weight: 256 });
  assert.equal(summary.akamai, fixture.connections[0].h2.akamai, "Chrome's measured preamble");
  const bad = new H2FrameTap();
  bad.push(Buffer.from("GET / HTTP/1.1\r\n\r\n------"));
  assert.equal(bad.error, "missing HTTP/2 client preface");
});

test("battery routes answer CORS, client-hint and Critical-CH requests", () => {
  const origins = { A: "https://127.0.0.1:1", AX: "https://localhost:1" };
  const cors = batteryRoute(
    { kind: "fetch-cors", tier: "chrome", headers: [["origin", origins.A]] },
    origins,
  );
  assert.equal(cors.headers["access-control-allow-origin"], origins.A);
  assert.equal(
    batteryRoute({ kind: "ch-accept", tier: "t", headers: [] }, origins).headers["accept-ch"],
    ACCEPT_CH,
  );
  const critical = batteryRoute({ kind: "critical", tier: "t", headers: [] }, origins);
  assert.equal(critical.headers["critical-ch"], "Sec-CH-UA-Full-Version-List");
  assert.match(
    batteryRoute({ kind: "nav", tier: "t", headers: [] }, origins).headers["set-cookie"],
    /Secure/,
  );
  assert.equal(batteryRoute({ kind: null, headers: [] }, origins).status, 404);
  assert.equal(labelize("https://localhost:1/t/render/k/x", origins), "<AX>/t/~/k/x");
  assert.equal(labelize("127.0.0.1:1", origins), "<A-host>");
  const request = normalizeRequest(
    {
      tier: "nojs",
      kind: "nav",
      method: "GET",
      path: "/t/nojs/k/nav",
      headers: [["referer", `${origins.A}/t/nojs/k/a`]],
    },
    origins,
  );
  assert.deepEqual(request.headers, [["referer", "<A>/t/~/k/a"]]);
});

test("parity diff classifies tells, environment noise, collector artifacts and known gaps", () => {
  const knownGaps = read("chrome-identity-known-gaps.json");
  // A turbo-surf capture that replays Chrome's requests exactly in both tiers.
  const asNative = (chrome) => ({
    ...clone(chrome),
    requests: [
      ...chrome.requests
        .filter((r) => NOJS_KINDS.includes(r.kind))
        .map((r) => ({ ...clone(r), tier: "nojs" })),
      ...chrome.requests
        .filter((r) => RENDER_KINDS.includes(r.kind))
        .map((r) => ({ ...clone(r), tier: "render" })),
    ],
  });
  assert.deepEqual(diffParity(fixture, asNative(fixture), { knownGaps }).deltas, []);
  const native = asNative(fixture);
  const nav = native.requests.find((request) => request.tier === "nojs");
  nav.headers.reverse();
  native.js.navigator.webdriver = false;
  native.js.navigator.hardwareConcurrency = 1;
  native.js.features.globals.push("__name");
  const { deltas } = diffParity(fixture, native, { knownGaps });
  const find = (predicate) => deltas.find(predicate);
  assert.equal(find((d) => d.type === "header-order" && d.tier === "nojs").class, "tell");
  assert.equal(find((d) => d.path === "navigator.hardwareConcurrency").class, "environment");
  assert.equal(find((d) => d.path === "navigator.webdriver").class, "collector");
  assert.equal(find((d) => d.path === "features.globals").gap, "window-global-inventory");
  native.js.features.globals.push("__newInternal");
  const again = diffParity(fixture, native, { knownGaps }).deltas;
  assert.equal(again.find((d) => d.path === "features.globals").class, "tell", "unlisted global");
  assert.equal(matches({ match: { header: "/^sec-/" } }, { header: "sec-ch-dpr" }), true);
});

test("every known gap is justified and still matched by the committed parity run", () => {
  const knownGaps = read("chrome-identity-known-gaps.json");
  const ids = new Set();
  for (const gap of knownGaps) {
    assert.ok(gap.id && gap.reason.length > 40 && gap.owner, gap.id);
    assert.ok(!ids.has(gap.id), `duplicate ${gap.id}`);
    ids.add(gap.id);
  }
  const transport = read(`TRANSPORT-${current.major}.json`);
  assert.equal(
    transport.identityParity.tell,
    undefined,
    "the committed run has no unexplained tell",
  );
  assert.deepEqual(transport.differences, []);
});

test("155→156-style version diff: data deltas update the registry, code deltas stop", () => {
  const legacy = read("fixtures/chrome-identity-155.json");
  assert.deepEqual(importLegacy(155), legacy, "the import is reproducible");
  const measuredDelta = diffVersions(legacy, fixture);
  assert.deepEqual(measuredDelta.errors, []);
  assert.deepEqual(blockers(measuredDelta), [], "the measured release needed no human");
  const next = clone(fixture);
  next.connections[0].hello.normalized.ciphers = [4865];
  next.connections[0].hello.ja4 = "t13d0000h2_000000000000_000000000000";
  const nav = next.requests.find((request) => request.kind === "nav");
  nav.headers.push(["sec-new-hint", "?1"]);
  nav.headers.find((pair) => pair[0] === "accept")[1] = "text/html";
  const stop = blockers(diffVersions(fixture, next), { transportBase: 154 }).map((b) => b.reason);
  assert.ok(stop.some((reason) => /TLS\/H2 changed/.test(reason)));
  assert.ok(stop.some((reason) => /new header sec-new-hint/.test(reason)));
  const actions = diffVersions(fixture, next).deltas.filter((d) => d.header === "accept");
  assert.equal(actions[0].action, "registry");
  const brands = clone(fixture);
  brands.requests
    .find((request) => request.kind === "nav")
    .headers.find((pair) => pair[0] === "sec-ch-ua")[1] = '"X";v="1"';
  assert.equal(diffVersions(legacy, brands).errors.length, 1, "GREASE algorithm change");
});

test("registry updates insert, replace and move the default without mutation", () => {
  const base = {
    defaultMajor: 156,
    releases: [{ major: 156, fullVersion: "156.0.1.2" }, { major: 155 }],
  };
  const added = applyRelease(base, { major: 157, fullVersion: "157.0.1.0" }, { fixture: "x" });
  assert.deepEqual(
    added.releases.map((entry) => entry.major),
    [157, 156, 155],
  );
  assert.equal(added.defaultMajor, 157);
  assert.equal(base.defaultMajor, 156);
  const replaced = applyRelease(base, { major: 156, fullVersion: "156.0.9.9" }, {});
  assert.deepEqual(
    replaced.releases.map((entry) => entry.fullVersion),
    ["156.0.9.9", undefined],
  );
  const old = applyRelease(base, { major: 150, fullVersion: "150.0.0.1" }, {});
  assert.equal(old.defaultMajor, 156, "an older measurement never becomes the default");
});

test("release status distinguishes newer stables from the measured build", () => {
  const full = current.fullVersion;
  const [major, minor, build, patch] = full.split(".").map(Number);
  assert.equal(compareVersions(full, full), 0);
  assert.equal(compareVersions("156.0.8078.47", "156.0.8078.5"), 1);
  assert.equal(releaseStatus(full, registry), "current");
  assert.equal(releaseStatus(`${major}.${minor}.${build}.${patch + 1}`, registry), "new-build");
  assert.equal(releaseStatus(`${major + 1}.0.0.0`, registry), "new-major");
  assert.equal(releaseStatus(`${major}.${minor}.${build - 1}.0`, registry), "older");
});

test("same-build drift ignores noise and blames a changed collector once", () => {
  assert.deepEqual(drift(fixture, clone(fixture)), []);
  const changed = clone(fixture);
  changed.collector["chrome-identity-probe.js"] = "0".repeat(64);
  assert.deepEqual(
    drift(fixture, changed).map((d) => d.type),
    ["collector-changed"],
  );
  const noisy = clone(fixture);
  noisy.js.screen.width += 1;
  assert.deepEqual(drift(fixture, noisy), []);
});

test("the optional engine step is skippable, tolerates absence and records its JSON", async () => {
  assert.deepEqual(await engineStep("156.0.8078.47", { skip: true }), { status: "skipped" });
  const missing = await engineStep("156.0.8078.47", { script: "/nonexistent/reversion_v8.py" });
  assert.equal(missing.status, "absent");
  const dir = mkdtempSync(join(tmpdir(), "turbo-engine-step-"));
  try {
    const script = join(dir, "reversion_v8.py");
    writeFileSync(
      script,
      "import json, sys\nprint(json.dumps({'argv': sys.argv[1:], 'v8': '15.6.75.19'}))\n",
    );
    chmodSync(script, 0o755);
    const ran = await engineStep("156.0.8078.47", { script, mode: "--resolve" });
    assert.equal(ran.status, "ran");
    assert.deepEqual(ran.report.argv, ["--chrome-version", "156.0.8078.47", "--resolve"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
