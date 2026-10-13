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

test("protocol headers pair repeated URLs by time and preserve ambiguous redirects", async () => {
  const { enrichProtocolHeaders } = await import("./protocol-headers.mjs");
  const requests = [1000, 2000].map((timeMs) => ({
    timeMs,
    url: "https://example.test/",
    method: "GET",
    headers: { accept: "old" },
  }));
  const main = (requestId, wallTime) => ({
    event: "requestWillBeSent",
    params: { requestId, wallTime, request: { url: "https://example.test/", method: "GET" } },
  });
  const extra = (requestId) => ({
    event: "requestWillBeSentExtraInfo",
    params: { requestId, headers: { "Sec-Fetch-Dest": "document", Cookie: "server=1" } },
  });
  enrichProtocolHeaders(requests, [main("b", 2), extra("b"), main("a", 1), extra("a")]);
  assert.deepEqual(
    requests.map((r) => r.cdpRequestId),
    ["a", "b"],
  );
  assert.equal(requestSummary(requests[0]).headerScope, "cdp-extra-info");
  assert.equal(requests[0].headers["sec-fetch-dest"], "document");
  assert.equal(requests[0].automationHeaders.accept, "old");
  const ambiguous = [{ timeMs: 1000, url: "https://example.test/", method: "GET" }];
  enrichProtocolHeaders(ambiguous, [main("r", 1), main("r", 1.1), extra("r")]);
  assert.equal(ambiguous[0].cdpRequestId, undefined);
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
