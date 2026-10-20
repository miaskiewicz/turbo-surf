import { test } from "node:test";
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareDirectory,
  interfaceDiff,
  requestSummary,
  nativeScriptInventory,
  captureSummary,
  chromeSearchOutcome,
  interactionSummary,
  observationSummary,
} from "./compare.mjs";

test("interaction evidence requires actual completed actions and type snapshots", () => {
  const trace = {
    config: { steps: [{ focus: "#q" }, { type: "fixture query" }, { press: "Enter" }] },
    actions: [
      { index: 0, action: "focus", completed: true },
      { index: 1, action: "type", completed: true },
      { index: 2, action: "press", completed: true },
    ],
    pages: [
      {
        phase: "step-1",
        inputState: { value: "fixture query", activeTag: "TEXTAREA", controlTag: "TEXTAREA" },
      },
    ],
  };
  const complete = interactionSummary(trace);
  assert.equal(complete.configuredActionsExecuted, true);
  assert.equal(complete.completedSteps, 3);
  assert.deepEqual(complete.typed, [
    { index: 1, valueMatchesTypedText: true, activeTagMatchesControl: true },
  ]);
  assert.equal(
    interactionSummary({ ...trace, actions: trace.actions.slice(0, 2) }).configuredActionsExecuted,
    false,
  );
  assert.equal(
    interactionSummary({ ...trace, actions: [...trace.actions].reverse() })
      .configuredActionsExecuted,
    false,
  );
  assert.equal(interactionSummary({ ...trace, pages: [] }).typed[0].valueMatchesTypedText, null);
  assert.equal(interactionSummary({ config: trace.config }).evidence, "unmeasured");
  const failed = structuredClone(trace);
  failed.actions.at(-1).completed = false;
  assert.equal(interactionSummary(failed).configuredActionsExecuted, false);
  assert.equal(interactionSummary(failed).completedSteps, 2);
  assert.equal(interactionSummary({ config: trace.config }).completedSteps, null);
  trace.pages[0].inputState.value = "unrelated-hidden-field";
  assert.equal(interactionSummary(trace).typed[0].valueMatchesTypedText, false);
});

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

test("collector completion stays independent of guest exceptions", () => {
  const errors = ["ReferenceError: page fixture is not defined"];
  const completed = {
    captureComplete: true,
    collectorErrors: [],
    pages: [{ phase: "search" }],
    errors,
  };
  const summary = captureSummary(completed);
  assert.equal(summary.complete, true);
  assert.equal(summary.completionSource, "collector");
  assert.equal(summary.collectorErrorCount, 0);
  assert.deepEqual(completed.errors, errors);
  assert.equal(captureSummary({ ...completed, captureComplete: false }).complete, false);
  assert.equal(
    captureSummary({ ...completed, collectorErrors: [{ phase: "target setup" }] }).complete,
    false,
  );
  const historical = captureSummary({ pages: completed.pages, errors });
  assert.equal(historical.complete, false);
  assert.equal(historical.completionSource, "legacy-error-heuristic");
  assert.equal(historical.collectorErrorCount, null);
});

test("native completion uses its actual host-error channel and rejects conflicting evidence", () => {
  const trace = {
    runtime: "turbo-surf",
    captureComplete: true,
    errors: [],
    pages: [{ phase: "native-final", errors: ["guest page exception"] }],
    observation: {
      policy: "fixed-deadline-before-enter",
      requestedMs: 100,
      elapsedMs: 101,
      overrunMs: 1,
      completed: true,
    },
  };
  assert.equal(captureSummary(trace).complete, true);
  assert.equal(captureSummary(trace).collectorErrorCount, 0);
  assert.equal(observationSummary(trace).complete, true);
  assert.equal(captureSummary({ ...trace, collectorErrors: [] }).complete, true);
  for (const errors of [undefined, null, {}, false, "", { length: 0 }, ["native host failure"]]) {
    const invalid = { ...trace, errors };
    assert.equal(captureSummary(invalid).complete, false);
    assert.equal(observationSummary(invalid).complete, false);
    assert.equal(captureSummary(invalid).collectorErrorCount, Array.isArray(errors) ? 1 : null);
  }
  const missing = { ...trace };
  delete missing.errors;
  assert.equal(captureSummary(missing).complete, false);
  for (const collectorErrors of [undefined, null, {}, false, ["conflicting failure"]]) {
    const invalid = { ...trace, collectorErrors };
    assert.equal(captureSummary(invalid).complete, false);
    assert.equal(observationSummary(invalid).complete, false);
  }
  assert.equal(captureSummary({ ...trace, runtime: "chrome" }).complete, false);
  assert.equal(captureSummary({ ...trace, runtime: "unknown" }).complete, false);
  assert.equal(
    captureSummary({
      ...trace,
      runtime: "chrome",
      collectorErrors: [],
      errors: ["guest exception"],
    }).complete,
    true,
  );
});

test("modern collector completion requires an actual empty error array", () => {
  const trace = {
    captureComplete: true,
    collectorErrors: [],
    config: { url: "https://www.google.com/" },
    pages: [
      {
        phase: "search",
        url: "https://www.google.com/search",
        status: 200,
        readyState: "complete",
        results: 1,
      },
    ],
    observation: {
      policy: "fixed-deadline-before-enter",
      requestedMs: 0,
      elapsedMs: 1,
      overrunMs: 1,
      completed: true,
    },
  };
  for (const collectorErrors of [{}, false, null, undefined, "", { length: 0 }]) {
    const malformed = { ...trace, collectorErrors };
    assert.equal(captureSummary(malformed).complete, false);
    assert.equal(captureSummary(malformed).collectorErrorCount, null);
    assert.equal(observationSummary(malformed).complete, false);
    assert.equal(chromeSearchOutcome(malformed).accepted, false);
  }
  assert.equal(captureSummary(trace).complete, true);
  assert.equal(observationSummary(trace).complete, true);
  assert.equal(chromeSearchOutcome(trace).accepted, true);
  const legacy = { pages: trace.pages, errors: [] };
  assert.equal(captureSummary(legacy).complete, true);
  assert.equal(captureSummary(legacy).completionSource, "legacy-error-heuristic");
  assert.equal(captureSummary(legacy).collectorErrorCount, null);
  assert.equal(observationSummary(legacy).evidence, "unmeasured");
  assert.equal(chromeSearchOutcome(legacy).accepted, false);
});

test("Chrome SERP verdict rejects HTTP 200 shells, challenges and incomplete documents", () => {
  const trace = {
    config: { url: "https://www.google.com/" },
    captureComplete: true,
    collectorErrors: [],
    errors: ["ReferenceError: independent guest fixture"],
    pages: [
      {
        phase: "search",
        url: "https://www.google.com/search?q=fixture",
        status: 200,
        readyState: "complete",
        results: 2,
      },
    ],
  };
  assert.equal(chromeSearchOutcome(trace).accepted, true);
  for (const change of [
    { results: 0 },
    { status: 429, url: "https://www.google.com/sorry/index" },
    { status: null },
    { readyState: "loading" },
    { phase: "step-5" },
    { url: "https://other.example/search" },
  ]) {
    const result = chromeSearchOutcome({ ...trace, pages: [{ ...trace.pages[0], ...change }] });
    assert.equal(result.accepted, false, JSON.stringify(change));
  }
  assert.equal(chromeSearchOutcome({ ...trace, captureComplete: false }).accepted, false);
  assert.equal(chromeSearchOutcome({ ...trace, collectorErrors: ["CDP timeout"] }).accepted, false);
  assert.equal(
    chromeSearchOutcome({ ...trace, config: { ...trace.config, successSelector: "body" } })
      .accepted,
    false,
  );
  assert.equal(chromeSearchOutcome({}).accepted, false);
  assert.equal(chromeSearchOutcome({}).classification, "unmeasured");
  assert.equal(chromeSearchOutcome({ pages: trace.pages }).evidence, "unmeasured");
});

test("runner comparison reports a measured challenge instead of a marker-only Chrome verdict", () => {
  const directory = mkdtempSync(join(tmpdir(), "detection-verdict-test-"));
  try {
    mkdirSync(join(directory, "chrome-baseline"));
    mkdirSync(join(directory, "native-baseline"));
    writeFileSync(
      join(directory, "chrome-baseline/trace.json"),
      JSON.stringify({
        config: { url: "https://www.google.com/" },
        captureComplete: true,
        collectorErrors: [],
        errors: ["ReferenceError: independent page fixture"],
        pages: [
          {
            phase: "search",
            url: "https://www.google.com/sorry/index",
            status: 429,
            readyState: "complete",
            results: 0,
          },
        ],
      }),
    );
    writeFileSync(join(directory, "native-baseline/trace.json"), JSON.stringify({ pages: [] }));
    const output = execFileSync(
      process.execPath,
      [fileURLToPath(new URL("./compare.mjs", import.meta.url)), directory],
      { encoding: "utf8" },
    );
    assert.match(output, /chrome SERP: challenge; accepted false; evidence measured/);
    assert.match(output, /chrome: 0 result markers; \/sorry\/index; capture complete/);
    const result = JSON.parse(readFileSync(join(directory, "comparison.json"), "utf8"));
    assert.equal(result.baseline.chromeSearchOutcome.accepted, false);
    assert.equal(result.baseline.capture.chrome.complete, true);
    assert.deepEqual(result.baseline.errors.chrome, ["ReferenceError: independent page fixture"]);
    assert.equal(result.interfaces.chromeSearchOutcome.classification, "unmeasured");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
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

test("fixed observation diagnostics never accept incomplete or pending navigation", () => {
  const trace = {
    config: { url: "https://www.google.com/", observationMs: 1000 },
    captureComplete: true,
    collectorErrors: [],
    observation: {
      policy: "fixed-deadline-before-enter",
      triggerStepIndex: 5,
      requestedMs: 1000,
      elapsedMs: 1010,
      overrunMs: 10,
      completed: true,
    },
    pages: [
      {
        phase: "search",
        url: "https://www.google.com/search?q=fixture",
        status: 200,
        readyState: "complete",
        results: 1,
      },
    ],
  };
  assert.equal(observationSummary(trace).complete, true);
  assert.equal(observationSummary(trace).overrunMs, 10);
  assert.equal(chromeSearchOutcome(trace).accepted, true);
  for (const change of [
    { completed: false },
    { elapsedMs: 500 },
    { navigationWaitTimedOut: true },
    { navigationFailed: true },
    { navigationHopLimit: true },
    { pendingNavigation: "https://www.google.com/sorry" },
    { pendingNavigation: 0 },
    { navigationFailed: "false" },
    { navigationWaitTimedOut: 0 },
    { navigationHopLimit: null },
    { requestedMs: 60001 },
    { elapsedMs: Infinity },
    { overrunMs: -1 },
  ]) {
    const altered = { ...trace, observation: { ...trace.observation, ...change } };
    assert.equal(observationSummary(altered).complete, false, JSON.stringify(change));
    assert.equal(chromeSearchOutcome(altered).accepted, false, JSON.stringify(change));
  }
  assert.equal(observationSummary({ ...trace, collectorErrors: ["CDP timeout"] }).complete, false);
  assert.equal(observationSummary({}).evidence, "unmeasured");
  assert.equal(chromeSearchOutcome({ ...trace, observation: undefined }).accepted, false);
});

test("an explicitly rootless final document is neither a complete observation nor a SERP", () => {
  const page = {
    phase: "search",
    url: "https://www.google.com/search",
    status: 200,
    readyState: "complete",
    results: 1,
    documentElementPresent: true,
  };
  const trace = {
    config: { url: "https://www.google.com/", observationMs: 0 },
    captureComplete: true,
    collectorErrors: [],
    pages: [page],
    observation: {
      policy: "fixed-deadline-before-enter",
      requestedMs: 0,
      elapsedMs: 1,
      overrunMs: 1,
      completed: true,
      documentElementPresent: true,
    },
  };
  assert.equal(chromeSearchOutcome(trace).accepted, true);
  assert.equal(observationSummary(trace).complete, true);
  for (const value of [false, null, "true", 0]) {
    const absent = { ...trace, pages: [page, { ...page, documentElementPresent: value }] };
    assert.equal(chromeSearchOutcome(absent).accepted, false);
    assert.equal(observationSummary(absent).complete, false);
    const invalidWindow = {
      ...trace,
      observation: { ...trace.observation, documentElementPresent: value },
    };
    assert.equal(observationSummary(invalidWindow).complete, false);
    assert.equal(chromeSearchOutcome(invalidWindow).accepted, false);
  }
  const rootless = {
    ...trace,
    pages: [page, { ...page, documentElementPresent: false, results: 0 }],
    observation: { ...trace.observation, completed: false, documentElementPresent: false },
  };
  assert.equal(captureSummary(rootless).complete, true); // Measurement succeeded.
  assert.equal(observationSummary(rootless).complete, false);
  assert.equal(chromeSearchOutcome(rootless).accepted, false);
  assert.equal(
    chromeSearchOutcome({ ...rootless, observation: undefined, config: { url: trace.config.url } })
      .accepted,
    false,
  );
  const legacy = structuredClone(trace);
  delete legacy.pages[0].documentElementPresent;
  delete legacy.observation.documentElementPresent;
  assert.equal(chromeSearchOutcome(legacy).accepted, true);
});
