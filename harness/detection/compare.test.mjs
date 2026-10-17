import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  compareDirectory,
  interfaceDiff,
  requestSummary,
  nativeScriptInventory,
  captureSummary,
} from "./compare.mjs";

test("partial captures expose input errors and page execution limits", () => {
  const summary = captureSummary({
    captureComplete: false,
    pages: [
      { phase: "home", hydration: { budget_exceeded: true }, inputState: { value: "" } },
      { phase: "interaction-error", error: "budget" },
    ],
  });
  assert.equal(summary.complete, false);
  assert.equal(summary.hydrationBudgetExceeded, true);
  assert.equal(summary.inputError, "budget");
  assert.deepEqual(summary.inputState, { value: "" });
  assert.equal(captureSummary({}).complete, false);
});

test("interface comparison retains value differences without double-counting snapshots", () => {
  const pages = (name, result, count) => [
    {
      interfaces: {
        accesses: [{ name, kind: "get", result, args: [], count, stack: "collector" }],
      },
    },
  ];
  const diff = interfaceDiff(
    [...pages("Navigator.userAgent", "chrome", 1), ...pages("Navigator.userAgent", "chrome", 2)],
    pages("navigator.userAgent", "native", 5),
  );
  assert.equal(diff.length, 1);
  assert.equal(diff[0].chromeCountMax, 2);
  assert.equal(diff[0].nativeCountMax, 5);
  assert.equal(diff[0].chromeOnlyValues[0].result, "chrome");
  assert.equal(diff[0].nativeOnlyValues[0].result, "native");
});
test("payload fields and explicit headers keep their scope", () => {
  const r = requestSummary({
    id: 1,
    url: "https://example.test/?sei=x&q=test&q=again",
    body: "token=a%26b&count=2",
    explicit_headers: { referer: "https://example.test/" },
  });
  assert.deepEqual(r.queryKeys, ["q", "sei"]);
  assert.deepEqual(r.bodyKeys, ["count", "token"]);
  assert.match(r.headerScope, /backend defaults absent/);
  assert.equal(r.bodyHash.length, 64);
});
test("partial runs preserve usable network observations and missing-trace failures", () => {
  const out = mkdtempSync(join(tmpdir(), "detection-diff-test-"));
  try {
    mkdirSync(join(out, "native-baseline"));
    writeFileSync(
      join(out, "native-baseline", "network.jsonl"),
      '{"event":"request","url":"https://example.test/","id":1}\n{"incomplete":',
    );
    const result = compareDirectory(out);
    assert.equal(result.baseline.network[0].native, 1);
    assert.equal(result.baseline.errors.chrome.length, 1);
    assert.equal(result.baseline.errors.native.length, 1);
    assert.equal(result.baseline.traceCoverage.incompleteNativeNetworkRecords, 1);
    assert.equal(JSON.parse(readFileSync(join(out, "comparison.json"))).baseline.network.length, 1);
    writeFileSync(join(out, "native-baseline", "trace.json"), '{"pages":[');
    const truncated = compareDirectory(out);
    assert.match(truncated.baseline.errors.native[0], /Incomplete trace.json/);
    assert.equal(truncated.baseline.capture.native.complete, false);
    assert.equal(truncated.baseline.network[0].native, 1);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("transport comparison keeps browser CDP and native application observations separate", () => {
  const out = mkdtempSync(join(tmpdir(), "transport-diff-test-"));
  try {
    mkdirSync(join(out, "chrome-baseline"));
    mkdirSync(join(out, "native-baseline"));
    writeFileSync(
      join(out, "chrome-baseline/trace.json"),
      JSON.stringify({
        networkCoverage: { source: "cdp-network", scope: "fixture" },
        responses: [
          { transport: { source: "cdp-response", protocol: "h2" } },
          { transport: { source: "unavailable" } },
        ],
      }),
    );
    writeFileSync(
      join(out, "native-baseline/network.jsonl"),
      [
        { event: "response", protocol: "HTTP/2.0", url: "https://example.test/" },
        { event: "response", url: "https://example.test/other" },
      ]
        .map(JSON.stringify)
        .join("\n"),
    );
    const result = compareDirectory(out).baseline;
    assert.equal(result.responseTransport.chrome.protocols.h2, 1);
    assert.equal(result.responseTransport.chrome.unknown, 1);
    assert.equal(result.responseTransport.native.protocols.h2, 1);
    assert.equal(result.responseTransport.native.unknown, 1);
    assert.equal(result.responseTransport.native.securityDetails, undefined);
    assert.equal(result.traceCoverage.chromeNetwork.source, "cdp-network");
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

const headerMain = (requestId, url, extra = {}) => ({
  event: "requestWillBeSent",
  ...extra,
  params: { requestId, wallTime: 1, request: { url, method: "GET" } },
});
const headerExtra = (requestId, value, extra = {}) => ({
  event: "requestWillBeSentExtraInfo",
  ...extra,
  params: { requestId, headers: { "X-Target": value, "Sec-Fetch-Dest": "document" } },
});

test("raw CDP headers use exact session IDs and redirect order rather than callback time", async () => {
  const { enrichProtocolHeaders } = await import("./protocol-headers.mjs");
  const url = "https://example.test/";
  const requests = [
    {
      protocolRequestId: "same",
      sessionId: "page",
      timeMs: 1005,
      url,
      method: "GET",
      headers: { accept: "page-old" },
    },
    {
      protocolRequestId: "same",
      sessionId: "worker",
      timeMs: 1000,
      url,
      method: "GET",
      headers: { accept: "worker-old" },
    },
    {
      protocolRequestId: "redirect",
      sessionId: "page",
      timeMs: 9000,
      url: url + "start",
      method: "GET",
    },
    {
      protocolRequestId: "redirect",
      sessionId: "page",
      timeMs: 500,
      url: url + "final",
      method: "GET",
    },
  ];
  enrichProtocolHeaders(requests, [
    headerMain("same", url, { sessionId: "page" }),
    headerMain("same", url, { sessionId: "worker" }),
    headerExtra("same", "worker", { sessionId: "worker" }),
    headerExtra("same", "page", { sessionId: "page" }),
    headerMain("redirect", url + "start", { sessionId: "page" }),
    headerExtra("redirect", "start", { sessionId: "page" }),
    headerMain("redirect", url + "final", { sessionId: "page" }),
    headerExtra("redirect", "final", { sessionId: "page" }),
  ]);
  assert.deepEqual(
    requests.map((row) => row.headers["x-target"]),
    ["page", "worker", "start", "final"],
  );
  assert.ok(requests.every((row) => row.headerScope === "cdp-extra-info"));
  assert.equal(requests[0].automationHeaders.accept, "page-old");
  assert.equal(requestSummary(requests[0]).headerScope, "cdp-extra-info");
});

test("Patchright headers enrich only unique main-frame navigation and never closer worker callbacks", async () => {
  const { enrichProtocolHeaders } = await import("./protocol-headers.mjs");
  const url = "https://example.test/";
  const requests = [
    {
      timeMs: 1005,
      timing: { startTime: 1005 },
      cdpTargetScope: "main-frame-navigation",
      url,
      method: "GET",
      headers: { accept: "main-old" },
    },
    {
      timeMs: 1000,
      cdpTargetScope: "unverified-target",
      url,
      method: "GET",
      headers: { accept: "worker-old" },
    },
  ];
  enrichProtocolHeaders(requests, [
    headerMain("page", url, { targetScope: "main-frame-navigation" }),
    headerExtra("page", "main"),
  ]);
  assert.equal(requests[0].headers["x-target"], "main");
  assert.equal(requests[0].headerScope, "cdp-extra-info");
  assert.equal(requests[1].headers.accept, "worker-old");
  assert.equal(requests[1].headerScope, "automation-api");
  assert.equal(requests[1].cdpRequestId, undefined);
  const unknown = [
    { ...requests[0], headers: { accept: "old" }, cdpRequestId: undefined, headerScope: undefined },
  ];
  enrichProtocolHeaders(unknown, [
    headerMain("child", url, { targetScope: "unverified-target" }),
    headerExtra("child", "child"),
  ]);
  assert.equal(unknown[0].headerScope, "automation-api");
  assert.equal(unknown[0].headers.accept, "old");
});

test("incomplete redirects, reordered raw hops and repeated Patchright navigation remain unjoined", async () => {
  const { enrichProtocolHeaders } = await import("./protocol-headers.mjs");
  const url = "https://example.test/";
  const raw = [url + "start", url + "final"].map((url) => ({
    protocolRequestId: "r",
    url,
    method: "GET",
    headers: { accept: "original" },
  }));
  const mains = [headerMain("r", url + "start"), headerMain("r", url + "final")];
  enrichProtocolHeaders(raw, [...mains, headerExtra("r", "incomplete")]);
  assert.ok(
    raw.every(
      (row) => row.headerScope === "cdp-request-event" && row.headers.accept === "original",
    ),
  );
  enrichProtocolHeaders(raw.toReversed(), [
    ...mains,
    headerExtra("r", "first"),
    headerExtra("r", "second"),
  ]);
  assert.ok(raw.every((row) => row.cdpRequestId === undefined));
  const repeated = [1, 2].map((time) => ({
    cdpTargetScope: "main-frame-navigation",
    timing: { startTime: time * 1000 },
    timeMs: time * 1000,
    url,
    method: "GET",
  }));
  enrichProtocolHeaders(repeated, [
    headerMain("a", url, { targetScope: "main-frame-navigation" }),
    headerExtra("a", "first"),
    headerMain("b", url, { targetScope: "main-frame-navigation" }),
    headerExtra("b", "second"),
  ]);
  assert.ok(
    repeated.every((row) => row.headerScope === "automation-api" && row.cdpRequestId === undefined),
  );
});

test("interface comparison separates native host-pump reads from page queries", () => {
  const pages = [
    {
      interfaces: {
        accesses: [
          {
            name: "CSSStyleSheet.cssRules",
            kind: "get",
            origin: "runtime",
            count: 6000,
            args: [],
            result: { type: "object" },
          },
          {
            name: "CSSStyleSheet.cssRules",
            kind: "get",
            origin: "page",
            count: 2,
            args: [],
            result: { type: "object" },
          },
        ],
      },
    },
  ];
  assert.equal(interfaceDiff([], pages)[0].nativeCountMax, 2);
  assert.equal(interfaceDiff([], pages, "runtime")[0].nativeCountMax, 6000);
});

test("native debugger inventory retains per-isolate coverage and incomplete capture", () => {
  const directory = mkdtempSync(join(tmpdir(), "turbo-inventory-"));
  try {
    assert.equal(nativeScriptInventory(directory).enabled, false);
    const isolate = join(directory, "native-js", "isolate-0");
    mkdirSync(isolate, { recursive: true });
    writeFileSync(
      join(isolate, "inventory.json"),
      JSON.stringify({
        coverage: "main and child contexts",
        scripts: [{ url: "<eval>", sourceHash: "abc" }],
        dropped: 2,
        pendingSources: 1,
        errors: ["source unavailable"],
      }),
    );
    const incomplete = join(directory, "native-js", "isolate-1");
    mkdirSync(incomplete);
    const result = nativeScriptInventory(directory);
    assert.equal(result.scripts, 1);
    assert.equal(result.dropped, 2);
    assert.equal(result.pendingSources, 1);
    assert.equal(result.isolates[0].coverage, "main and child contexts");
    assert.equal(result.errors.length, 2);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
