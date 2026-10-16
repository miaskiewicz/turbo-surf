import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { summarizeRun } from "./summarize-run.mjs";

function fixture(body) {
  const directory = mkdtempSync(join(tmpdir(), "capture-summary-"));
  const write = (name, value) => writeFileSync(join(directory, name), JSON.stringify(value));
  mkdirSync(join(directory, "native-baseline"));
  writeFileSync(join(directory, "native-executable"), "frozen executable");
  write("manifest.json", {
    binaryHash: createHash("sha256").update("frozen executable").digest("hex"),
    runs: [{ runtime: "native", pass: "baseline", repeat: 0, code: 0 }],
  });
  write("baseline-config.json", { disableFormFallback: true, query: "private-query" });
  write("native-baseline/trace.json", {
    captureComplete: true,
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
    assert.deepEqual(result.nativeBaseline, { captured: 1, successful: 1 });
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
