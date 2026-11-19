import assert from "node:assert/strict";

const key = (item) => JSON.stringify([item.file, ...item.title_path]);
const titleKey = (item) => JSON.stringify(item.title_path);
const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const ratio = (chrome, surf) => (surf > 0 && chrome > 0 ? chrome / surf : null);

function expectedIndex(expected) {
  assert.equal(expected.length, 46, "Declare all 46 original auth cases");
  const titles = new Map();
  for (const item of expected) {
    assert.ok(item.file && item.title_path?.length, "Missing expected case identity");
    assert.ok(
      !titles.has(titleKey(item)),
      "TAP cannot disambiguate duplicate title paths across files",
    );
    titles.set(titleKey(item), item);
  }
  return titles;
}

function add(rows, item, duration) {
  assert.ok(Number.isFinite(duration) && duration >= 0, "Invalid case duration");
  assert.ok(!rows.has(key(item)), "Duplicate case outcome");
  rows.set(key(item), duration);
}

function tapDetails(lines, start, depth) {
  const block = [];
  for (const line of lines.slice(start + 1)) {
    if (line.trim() === "...") break;
    if (line.trim() && line.length - line.trimStart().length <= depth) break;
    block.push(line);
  }
  const text = block.join("\n");
  return {
    leaf: /^\s*type: ['"]?test['"]?\s*$/m.test(text),
    duration: Number(/^\s*duration_ms:\s*([\d.e+-]+)\s*$/m.exec(text)?.[1]),
  };
}

export function parseAuthTap(text, expected) {
  const titles = expectedIndex(expected);
  const headers = new Map();
  const rows = new Map();
  const lines = text.split("\n");
  for (const [index, line] of lines.entries()) {
    assert.ok(!/^\s*(?:not ok\b|Bail out!)/i.test(line), "Non-passing TAP outcome");
    const header = /^(\s*)# Subtest: (.*)$/.exec(line);
    if (header) {
      for (const depth of headers.keys()) if (depth >= header[1].length) headers.delete(depth);
      headers.set(header[1].length, header[2]);
    }
    const status = /^(\s*)ok \d+ - (.*)$/.exec(line);
    if (!status) continue;
    assert.ok(!/\s#\s*(?:SKIP|TODO)\b/i.test(status[2]), "Skipped/TODO TAP case");
    const details = tapDetails(lines, index, status[1].length);
    if (!details.leaf) continue;
    assert.equal(headers.get(status[1].length), status[2], "TAP subtest/result identity mismatch");
    const path = [...headers]
      .filter(([depth]) => depth < status[1].length)
      .sort(([a], [b]) => a - b)
      .map(([, title]) => title);
    path.push(status[2]);
    const item = titles.get(JSON.stringify(path));
    assert.ok(item, `Unexpected TAP case: ${JSON.stringify(path)}`);
    add(rows, item, details.duration);
  }
  for (const [name, count] of Object.entries({
    tests: 46,
    pass: 46,
    fail: 0,
    skipped: 0,
    cancelled: 0,
    todo: 0,
  })) {
    const values = [...text.matchAll(new RegExp(`^# ${name} (\\d+)$`, "gm"))];
    assert.equal(values.length, 1, `Missing/duplicate TAP summary: ${name}`);
    assert.equal(Number(values[0][1]), count, `Unexpected TAP summary: ${name}`);
  }
  assert.equal(rows.size, 46, "Missing TAP cases");
  return rows;
}

function chromeSpec(spec, names, expected, rows) {
  for (const test of spec.tests ?? []) {
    assert.equal(spec.ok, true, "Failed Chrome spec");
    assert.equal(test.expectedStatus, "passed", "Expected-failure Chrome case");
    assert.equal(test.status, "expected", "Flaky/unexpected Chrome test");
    assert.equal(test.results?.length, 1, "Chrome retry/missing result");
    assert.equal(test.results[0].status, "passed", "Non-passing Chrome result");
    assert.equal(test.results[0].retry, 0, "Retried Chrome result");
    assert.equal(test.results[0].errors?.length ?? 0, 0, "Chrome result errors");
    const path = [...names, spec.title];
    const candidates = expected.filter(
      (item) =>
        titleKey(item) === JSON.stringify(path) &&
        item.project === test.projectName &&
        (spec.file === item.file ||
          `e2e/${spec.file}` === item.file ||
          spec.file.endsWith(`/${item.file}`)),
    );
    assert.equal(candidates.length, 1, "Unexpected/ambiguous Chrome identity");
    if (candidates[0].id !== undefined) {
      assert.equal(
        candidates[0].id,
        `${test.projectName}:${spec.id}`,
        "Chrome collected ID mismatch",
      );
    }
    add(rows, candidates[0], test.results[0].duration);
  }
}

export function parseAuthChrome(report, expected) {
  expectedIndex(expected);
  assert.equal(report.errors?.length ?? 0, 0, "Chrome reporter errors");
  const rows = new Map();
  const visit = (suites, parents) => {
    for (const suite of suites) {
      const names = suite.title.endsWith(".spec.ts") ? parents : [...parents, suite.title];
      for (const spec of suite.specs ?? []) chromeSpec(spec, names, expected, rows);
      visit(suite.suites ?? [], names);
    }
  };
  visit(report.suites ?? [], []);
  assert.equal(rows.size, 46, "Missing Chrome cases");
  return rows;
}

function processEvidence(run) {
  assert.equal(run.exitCode, 0, "Nonzero/missing process exit");
  assert.equal(run.signal ?? null, null, "Signaled process");
  assert.equal(run.instrumented, false, "Instrumented timing is diagnostic only");
  assert.equal(run.suspensionDetected, false, "Suspended/unchecked run");
  assert.equal(run.contended, false, "Contended/unchecked run");
  assert.equal(run.workers, 1, "Auth comparison requires one worker");
  assert.ok(Number.isFinite(run.wallMs) && run.wallMs > 0, "Invalid complete command wall time");
  assert.ok(Number.isFinite(run.clockGapMs) && run.clockGapMs >= 0, "Missing clock-gap evidence");
  assert.ok(
    Number.isFinite(run.clockGapLimitMs) && run.clockGapLimitMs >= 0,
    "Missing predeclared clock-gap limit",
  );
  assert.ok(
    run.clockGapMs <= run.clockGapLimitMs,
    "Host clock gap exceeds declared measurement limit",
  );
  const start = Date.parse(run.startedAt);
  const end = Date.parse(run.endedAt);
  assert.ok(Number.isFinite(start) && end > start, "Missing/invalid invocation timestamps");
  assert.ok(
    Math.abs(end - start - run.wallMs) <= run.clockGapLimitMs,
    "Wall/monotonic clock discrepancy",
  );
  for (const field of [
    "cachePolicy",
    "coldWarmPolicy",
    "fixtureReset",
    "setupNotes",
    "provenance",
  ]) {
    assert.ok(typeof run[field] === "string" && run[field].trim(), `Missing ${field}`);
  }
}

export function compareAuthTrials(expected, pairs) {
  expectedIndex(expected);
  assert.ok(pairs.length, "No independent matched trials");
  const observations = pairs.map(({ surf, chrome }) => {
    processEvidence(surf);
    processEvidence(chrome);
    assert.equal(surf.optimized, true, "Surf must use an optimized immutable addon");
    for (const field of ["cachePolicy", "coldWarmPolicy", "fixtureReset"]) {
      assert.equal(surf[field], chrome[field], `Unmatched ${field}`);
    }
    const surfRows = parseAuthTap(surf.tap, expected);
    const chromeRows = parseAuthChrome(chrome.report, expected);
    assert.deepEqual(
      [...surfRows.keys()].sort(),
      [...chromeRows.keys()].sort(),
      "Workload mismatch",
    );
    return { surf, chrome, surfRows, chromeRows };
  });
  const intervals = observations
    .flatMap(({ surf, chrome }) => [surf, chrome])
    .sort((a, b) => Date.parse(a.startedAt) - Date.parse(b.startedAt));
  for (let index = 1; index < intervals.length; index++) {
    assert.ok(
      Date.parse(intervals[index].startedAt) >= Date.parse(intervals[index - 1].endedAt),
      "Matched timing invocations overlap",
    );
  }
  const surfWall = observations.map(({ surf }) => surf.wallMs);
  const chromeWall = observations.map(({ chrome }) => chrome.wallMs);
  const elapsedSum = (rows) => [...rows.values()].reduce((total, value) => total + value, 0);
  return {
    scope:
      "46 original auth cases; elapsed times include IO, waits, setup and cleanup, not CPU attribution",
    trials: observations.length,
    testElapsedSum: {
      surfMedianMs: median(observations.map(({ surfRows }) => elapsedSum(surfRows))),
      chromeMedianMs: median(observations.map(({ chromeRows }) => elapsedSum(chromeRows))),
      interpretation: "Descriptive case elapsed sum; not complete command wall time or CPU time",
    },
    wall: {
      surfMedianMs: median(surfWall),
      chromeMedianMs: median(chromeWall),
      pairedMedianSpeedup: median(
        observations.map(({ surf, chrome }) => chrome.wallMs / surf.wallMs),
      ),
      surfRangeMs: [Math.min(...surfWall), Math.max(...surfWall)],
      chromeRangeMs: [Math.min(...chromeWall), Math.max(...chromeWall)],
    },
    cases: expected.map((item) => {
      const surfMs = median(observations.map(({ surfRows }) => surfRows.get(key(item))));
      const chromeMs = median(observations.map(({ chromeRows }) => chromeRows.get(key(item))));
      return {
        ...item,
        surfMedianMs: surfMs,
        chromeMedianMs: chromeMs,
        speedup: ratio(chromeMs, surfMs),
        resolutionLimited: !surfMs || !chromeMs,
      };
    }),
  };
}
