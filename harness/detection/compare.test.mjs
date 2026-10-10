import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { compareDirectory, interfaceDiff, requestSummary } from "./compare.mjs";

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
