import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { processSummary, summarizeRun } from "./summarize-run.mjs";

test("fresh process evidence rejects abort, output failure and malformed flags even with complete results", () => {
  fixture((directory, write) => {
    const manifest = JSON.parse(readFileSync(join(directory, "manifest.json")));
    const original = { ...manifest.runs[0] };
    const secret = "PRIVATE_PROCESS_ERROR_7319";
    for (const change of [
      { aborted: true },
      { aborted: 0 },
      { aborted: null },
      { timedOut: true },
      { timedOut: 0 },
      { timedOut: null },
      { error: "https://private.invalid/?token=" + secret },
      { error: "" },
      { error: false },
      { error: {} },
      { signal: "SIGTERM" },
      { signal: secret },
      { signal: false },
      { signal: {} },
      { signal: [] },
      { code: false },
    ]) {
      manifest.runs[0] = { ...original, ...change };
      write("manifest.json", manifest);
      const result = summarizeRun(directory);
      assert.equal(result.nativeBaseline.successful, 0);
      assert.equal(result.runs["0/native-baseline"].process.complete, false);
      assert.doesNotMatch(JSON.stringify(result), /PRIVATE_PROCESS_ERROR_7319|private\.invalid/);
    }
    for (const key of ["timedOut", "aborted", "error", "signal", "code"]) {
      manifest.runs[0] = { ...original };
      delete manifest.runs[0][key];
      write("manifest.json", manifest);
      const result = summarizeRun(directory);
      assert.equal(result.nativeBaseline.processMeasured, 0);
      assert.equal(result.nativeBaseline.successful, 0);
      assert.equal(result.runs["0/native-baseline"].process.evidence, "unmeasured");
      assert.equal(result.runs["0/native-baseline"].process.complete, null);
    }
    manifest.runs[0] = original;
    write("manifest.json", manifest);
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 1);
  });
});

test("malformed collector error evidence cannot become a successful public baseline", () => {
  fixture((directory, write) => {
    const path = "native-baseline/trace.json";
    const original = JSON.parse(readFileSync(join(directory, path)));
    for (const collectorErrors of [{}, false, null, undefined, "", { length: 0 }]) {
      write(path, { ...original, collectorErrors });
      const summary = summarizeRun(directory);
      const run = summary.runs["0/native-baseline"];
      assert.equal(run.collectorComplete, false);
      assert.equal(run.collectorErrorCount, null);
      assert.equal(run.observation.complete, false);
      assert.equal(summary.nativeBaseline.successful, 0);
    }
    write(path, original);
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 1);
    const legacy = { ...original };
    for (const key of ["captureComplete", "collectorErrors", "observation"]) delete legacy[key];
    write(path, { ...legacy, errors: [] });
    const historical = summarizeRun(directory);
    assert.equal(
      historical.runs["0/native-baseline"].collectorCompletionSource,
      "legacy-error-heuristic",
    );
    assert.equal(historical.runs["0/native-baseline"].observation.evidence, "unmeasured");
    assert.equal(historical.nativeBaseline.successful, 0);
  });
});

test("historical process rows remain unmeasured rather than silently becoming fresh successes", () => {
  const terminated = processSummary({
    code: null,
    timedOut: false,
    aborted: true,
    error: null,
    signal: "SIGTERM",
  });
  assert.equal(terminated.evidence, "measured");
  assert.equal(terminated.complete, false);
  assert.deepEqual(processSummary({ code: 0, timedOut: false }), {
    evidence: "unmeasured",
    complete: null,
    exitCode: 0,
    timedOut: false,
    aborted: null,
    errorPresent: null,
    terminationSignal: null,
  });
  assert.equal(
    processSummary({ code: 0, timedOut: false, aborted: false, error: null, signal: null })
      .complete,
    true,
  );
});

function fixture(body) {
  const directory = mkdtempSync(join(tmpdir(), "capture-summary-"));
  const write = (name, value) => writeFileSync(join(directory, name), JSON.stringify(value));
  mkdirSync(join(directory, "native-baseline"));
  writeFileSync(join(directory, "native-executable"), "frozen executable");
  write("manifest.json", {
    binaryHash: createHash("sha256").update("frozen executable").digest("hex"),
    runs: [
      {
        runtime: "native",
        pass: "baseline",
        repeat: 0,
        code: 0,
        timedOut: false,
        aborted: false,
        error: null,
        signal: null,
      },
    ],
  });
  write("baseline-config.json", {
    url: "https://www.google.com/",
    disableFormFallback: true,
    query: "private-query",
  });
  write("native-baseline/trace.json", {
    captureComplete: true,
    collectorErrors: [],
    windowTimerMode: "real-time",
    observation: {
      policy: "fixed-deadline-before-enter",
      triggerStepIndex: 5,
      requestedMs: 1000,
      elapsedMs: 1001.5,
      overrunMs: 1.5,
      completed: true,
    },
    navigationSource: "native-form",
    errors: ["private-error"],
    pages: [
      {
        phase: "native-final",
        url: "https://www.google.com/search?q=private-query",
        status: 200,
        readyState: "complete",
        hydration: { budget_exceeded: false },
        nav: "https://www.google.com/search?q=private-query",
        results: 9,
        cookies: [{ name: "BUCKET", value: "private-cookie" }],
      },
    ],
  });
  try {
    body(directory, write);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test("public evidence excludes cookie values, queries, bodies and diagnostics while preserving consent order", () => {
  fixture((directory) => {
    const rows = [
      { event: "response", url: "https://google.com/save", status: 204, time_us: 1 },
      {
        event: "request",
        url: "https://www.google.com/search?q=private-query",
        time_us: 2,
        method: "GET",
        body: "private-body\u2028still-one-json-record",
        explicit_headers: { Cookie: "BUCKET=private-cookie", "Sec-Fetch-User": "?1" },
      },
    ];
    writeFileSync(
      join(directory, "native-baseline/network.jsonl"),
      rows.map(JSON.stringify).join("\n"),
    );
    const result = summarizeRun(directory);
    assert.deepEqual(result.nativeBaseline, {
      captured: 1,
      processMeasured: 1,
      observationMeasured: 1,
      successful: 1,
    });
    const run = result.runs["0/native-baseline"];
    assert.equal(run.consent.responseBeforeFirstSearch, true);
    assert.deepEqual(run.navigationRequests[0].cookieNames, ["BUCKET"]);
    assert.equal(run.recordedErrorCount, 1);
    assert.doesNotMatch(JSON.stringify(result), /private-|still-one-json-record/);
  });
});

test("fallback, incomplete or failed captures cannot count as native acceptance", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    for (const change of [
      { captureComplete: false },
      { formFallback: { url: "https://www.google.com/search" } },
    ]) {
      write("native-baseline/trace.json", {
        ...trace,
        ...change,
      });
      assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    }
    write("native-baseline/trace.json", trace);
    write("baseline-config.json", { disableFormFallback: false });
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    write("baseline-config.json", { disableFormFallback: true });
    const manifest = JSON.parse(readFileSync(join(directory, "manifest.json")));
    manifest.runs[0].code = 1;
    write("manifest.json", manifest);
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    rmSync(join(directory, "native-baseline/trace.json"));
    const missing = summarizeRun(directory);
    assert.equal(missing.runs["0/native-baseline"].traceMissing, true);
    assert.equal(missing.nativeBaseline.successful, 0);
  });
});

test("summaries reject executable drift and malformed JSONL instead of hiding lost evidence", () => {
  fixture((directory) => {
    writeFileSync(join(directory, "native-baseline/network.jsonl"), '{"event":');
    assert.throws(() => summarizeRun(directory), SyntaxError);
    writeFileSync(join(directory, "native-executable"), "different executable");
    assert.throws(() => summarizeRun(directory), /differs from the manifest/);
  });
});

test("only the settled final successful document counts, never intermediate results or unfinished navigation", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    const page = trace.pages[0];
    for (const change of [
      { status: 429 },
      { readyState: "loading" },
      { hydration: { budget_exceeded: true } },
      { hydration: {} },
      { nav: undefined },
      { nav: "https://www.google.com/sorry/index?secret=private-token" },
      { results: "9" },
      { results: 0 },
      { phase: "navigation-0-error" },
    ]) {
      write("native-baseline/trace.json", { ...trace, pages: [{ ...page, ...change }] });
      assert.equal(summarizeRun(directory).nativeBaseline.successful, 0, JSON.stringify(change));
    }
    write("native-baseline/trace.json", {
      ...trace,
      pages: [
        page,
        {
          ...page,
          phase: "navigation-0",
          url: "https://www.google.com/sorry/index",
          nav: "https://www.google.com/sorry/index",
          status: 429,
        },
      ],
    });
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    write("native-baseline/trace.json", {
      ...trace,
      pages: [page, { ...page, phase: "navigation-0" }],
    });
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 1);
    write("native-baseline/trace.json", {
      ...trace,
      pages: [
        {
          ...page,
          url: "https://www.google.com/private-path-token",
          nav: "https://www.google.com/private-path-token",
        },
      ],
    });
    const unknown = summarizeRun(directory);
    assert.equal(unknown.runs["0/native-baseline"].pages[0].path, "<other>");
    assert.equal(unknown.nativeBaseline.successful, 0);
    assert.doesNotMatch(JSON.stringify(unknown), /private-path-token|private-query|private-token/);
  });
});

test("timeouts and absent or incomplete consent timing never invent successful evidence", () => {
  fixture((directory, write) => {
    const manifest = JSON.parse(readFileSync(join(directory, "manifest.json")));
    manifest.runs[0].timedOut = true;
    write("manifest.json", manifest);
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    manifest.runs[0].timedOut = false;
    delete manifest.runs[0].code;
    write("manifest.json", manifest);
    assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    assert.equal(
      summarizeRun(directory).runs["0/native-baseline"].consent.responseBeforeFirstSearch,
      null,
    );
    writeFileSync(
      join(directory, "native-baseline/network.jsonl"),
      [
        { event: "response", url: "https://google.com/save", status: 204 },
        { event: "request", url: "https://www.google.com/search", time_us: 2 },
      ]
        .map(JSON.stringify)
        .join("\n"),
    );
    const partial = summarizeRun(directory).runs["0/native-baseline"];
    assert.deepEqual(partial.consent.responseStatuses, [204]);
    assert.equal(partial.consent.responseBeforeFirstSearch, null);
  });
});

test("Google acceptance excludes other origins and custom starting URLs even with search markers", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    const page = trace.pages[0];
    for (const url of [
      "https://other.invalid/search?q=private-query",
      "http://www.google.com/search",
      "https://www.google.com.other.invalid/search",
    ]) {
      write("native-baseline/trace.json", { ...trace, pages: [{ ...page, url, nav: url }] });
      const result = summarizeRun(directory);
      assert.equal(result.nativeBaseline.successful, 0);
      assert.equal(result.runs["0/native-baseline"].pages[0].origin, "<other>");
      assert.doesNotMatch(JSON.stringify(result), /other\.invalid|private-query/);
    }
    write("native-baseline/trace.json", trace);
    for (const url of ["http://localhost/", "https://other.invalid/", undefined]) {
      write("baseline-config.json", { url, disableFormFallback: true });
      assert.equal(summarizeRun(directory).nativeBaseline.successful, 0);
    }
    write("baseline-config.json", { url: "https://www.google.com/", disableFormFallback: true });
    const good = summarizeRun(directory);
    assert.equal(good.nativeBaseline.successful, 1);
    assert.equal(good.runs["0/native-baseline"].startOrigin, "https://www.google.com");
  });
});

test("public transport evidence preserves negotiated counts without leaking raw CDP metadata", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    trace.responses = [
      {
        url: "https://www.google.com/private-endpoint?token=private-query",
        transport: {
          source: "cdp-response",
          protocol: "h2",
          securityDetails: {
            protocol: "TLS 1.3",
            subjectName: "private-certificate",
            sanList: ["private-host"],
          },
          timing: { requestTime: 123456789, sslStart: 4, sslEnd: 7 },
          connectionId: 987654321,
          remoteIPAddress: "192.0.2.123",
          remotePort: 443,
          connectionReused: true,
          fromDiskCache: false,
          fromServiceWorker: false,
        },
      },
      {
        url: "https://www.google.com/",
        transport: { source: "cdp-response", protocol: "private-protocol" },
      },
      { url: "https://www.google.com/" },
    ];
    const manifest = JSON.parse(readFileSync(join(directory, "manifest.json")));
    manifest.runs[0].runtime = "chrome";
    write("manifest.json", manifest);
    mkdirSync(join(directory, "chrome-baseline"));
    write("chrome-baseline/trace.json", trace);
    const result = summarizeRun(directory);
    assert.equal(result.schemaVersion, 6);
    const transport = result.runs["0/chrome-baseline"].responseTransport;
    assert.equal(transport.responses, 3);
    assert.equal(transport.observed, 2);
    assert.equal(transport.unknown, 1);
    assert.equal(transport.protocols.h2, 1);
    assert.equal(transport.protocols.other, 1);
    assert.equal(transport.protocols.unknown, 1);
    assert.equal(transport.tlsProtocols["TLS 1.3"], 1);
    assert.deepEqual(transport.securityDetails, { present: 1, missing: 2 });
    assert.deepEqual(transport.timing, { present: 1, missing: 2 });
    assert.deepEqual(transport.connectionReused, { true: 1, false: 0, unknown: 2 });
    assert.doesNotMatch(
      JSON.stringify(result),
      /private-|192\.0\.2\.123|123456789|987654321|remotePort|sanList|subjectName/,
    );
  });
});

test("old captures expose missing transport and observation evidence without guessing acceptance", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    delete trace.observation;
    delete trace.windowTimerMode;
    write("native-baseline/trace.json", trace);
    const result = summarizeRun(directory);
    const transport = result.runs["0/native-baseline"].responseTransport;
    assert.equal(transport.responses, 0);
    assert.equal(transport.observed, 0);
    assert.equal(transport.protocols.h3, 0);
    assert.equal(transport.tlsDetails, "not-collected");
    assert.equal(transport.source, "native-application-response");
    assert.equal(result.runs["0/native-baseline"].observation.evidence, "unmeasured");
    assert.equal(result.nativeBaseline.observationMeasured, 0);
    assert.equal(result.runs["0/native-baseline"].observation.complete, null);
    assert.equal(result.runs["0/native-baseline"].windowTimerMode, null);
    assert.equal(result.nativeBaseline.successful, 0);
  });
});

test("native protocol aggregation reads only application response observations", () => {
  fixture((directory) => {
    writeFileSync(
      join(directory, "native-baseline/network.jsonl"),
      [
        { event: "response", protocol: "HTTP/2.0", url: "https://www.google.com/" },
        { event: "response", protocol: "private-protocol", url: "https://www.google.com/" },
        { event: "response", url: "https://www.google.com/" },
      ]
        .map(JSON.stringify)
        .join("\n"),
    );
    const result = summarizeRun(directory);
    const transport = result.runs["0/native-baseline"].responseTransport;
    assert.equal(transport.source, "native-application-response");
    assert.equal(transport.protocols.h2, 1);
    assert.equal(transport.protocols.other, 1);
    assert.equal(transport.protocols.unknown, 1);
    assert.equal(transport.tlsDetails, "not-collected");
    assert.doesNotMatch(JSON.stringify(result), /private-protocol/);
  });
});

test("hydration progress exposes only known native phases and valid pump counts", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    trace.pages[0].hydration.progress = {
      phase: "parser-and-deferred-scripts",
      completed_pumps: 0,
      source: "private-source",
    };
    write("native-baseline/trace.json", trace);
    assert.deepEqual(summarizeRun(directory).runs["0/native-baseline"].pages[0].hydrationProgress, {
      phase: "parser-and-deferred-scripts",
      completedPumps: 0,
    });
    trace.pages[0].hydration.progress = {
      phase: "private-script-name",
      completed_pumps: -1,
    };
    write("native-baseline/trace.json", trace);
    const result = summarizeRun(directory);
    assert.deepEqual(result.runs["0/native-baseline"].pages[0].hydrationProgress, {
      phase: "unknown",
      completedPumps: null,
    });
    assert.doesNotMatch(JSON.stringify(result), /private-/);
  });
});

test("a fixed-window native result requires complete observation and genuine real-time mode", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    for (const change of [
      {
        observation: {
          ...trace.observation,
          pendingNavigation: "https://www.google.com/?private-token",
        },
      },
      { observation: { ...trace.observation, navigationFailed: true } },
      { observation: { ...trace.observation, navigationWaitTimedOut: true } },
      { observation: { ...trace.observation, navigationHopLimit: true } },
      { observation: { ...trace.observation, completed: false } },
      { observation: { ...trace.observation, completed: 1 } },
      { observation: { ...trace.observation, elapsedMs: 999 } },
      { collectorErrors: [{ error: "private-collector-error" }] },
      { captureComplete: 1 },
      { windowTimerMode: "virtual" },
      { windowTimerMode: undefined },
    ]) {
      write("native-baseline/trace.json", { ...trace, ...change });
      const result = summarizeRun(directory);
      assert.equal(result.nativeBaseline.successful, 0, JSON.stringify(change));
      assert.doesNotMatch(JSON.stringify(result), /private-token|private-collector-error/);
    }
  });
});

test("public observation evidence excludes pending URLs, request identifiers and absolute timestamps", () => {
  fixture((directory, write) => {
    const trace = JSON.parse(readFileSync(join(directory, "native-baseline/trace.json")));
    trace.observation = {
      ...trace.observation,
      startedMs: 123456789,
      deadlineMs: 123457789,
      finishedMs: 123457791,
      pendingNavigation: "https://192.0.2.7/?q=private-token",
      pendingNavigationRequest: {
        url: "https://192.0.2.7/?q=private-token",
        requestId: "private-request",
        loaderId: "private-loader",
      },
      failedNavigationRequest: { errorText: "private-error-text" },
    };
    write("native-baseline/trace.json", trace);
    const run = summarizeRun(directory).runs["0/native-baseline"];
    assert.equal(run.observation.pendingNavigation, true);
    assert.equal(run.observation.complete, false);
    assert.equal(run.observation.requestedMs, 1000);
    assert.equal(run.observation.elapsedMs, 1001.5);
    assert.equal(run.windowTimerMode, "real-time");
    assert.equal(run.collectorComplete, true);
    assert.equal(run.collectorCompletionSource, "collector");
    assert.equal(run.collectorErrorCount, 0);
    assert.doesNotMatch(
      JSON.stringify(run.observation),
      /private-|192\.0\.2\.7|123456789|123457789|123457791|startedMs|deadlineMs|finishedMs|requestId|loaderId|errorText/,
    );
    trace.navigationSource = "script";
    write("native-baseline/trace.json", trace);
    assert.equal(summarizeRun(directory).runs["0/native-baseline"].navigationSource, "script");
  });
});
