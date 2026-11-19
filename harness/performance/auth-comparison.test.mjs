import assert from "node:assert/strict";
import test from "node:test";
import { compareAuthTrials, parseAuthChrome, parseAuthTap } from "./auth-comparison.mjs";

// Synthetic protocol fixtures only; these are never performance evidence.
const expected = Array.from({ length: 46 }, (_, index) => ({
  file: "e2e/src/auth/example.spec.ts",
  title_path: ["auth", `case ${index}`],
  project: "chromium",
}));
const tap =
  "# Subtest: auth\n" +
  expected
    .map(
      (item) =>
        `    # Subtest: ${item.title_path[1]}\n    ok 1 - ${item.title_path[1]}\n      ---\n      duration_ms: 2\n      type: 'test'\n      ...\n`,
    )
    .join("") +
  "ok 1 - auth\n# tests 46\n# pass 46\n# fail 0\n# skipped 0\n# cancelled 0\n# todo 0\n";
const chrome = {
  errors: [],
  suites: [
    {
      title: "example.spec.ts",
      suites: [
        {
          title: "auth",
          specs: expected.map((item) => ({
            title: item.title_path[1],
            file: "src/auth/example.spec.ts",
            ok: true,
            tests: [
              {
                projectName: "chromium",
                expectedStatus: "passed",
                status: "expected",
                results: [{ status: "passed", duration: 4, retry: 0, errors: [] }],
              },
            ],
          })),
        },
      ],
    },
  ],
};
const run = () => ({
  exitCode: 0,
  signal: null,
  instrumented: false,
  suspensionDetected: false,
  optimized: true,
  contended: false,
  workers: 1,
  wallMs: 200,
  clockGapMs: 0,
  clockGapLimitMs: 100,
  cachePolicy: "fresh context",
  coldWarmPolicy: "warm app",
  fixtureReset: "same seed reset",
  setupNotes: "setup and teardown included",
  provenance: "synthetic test only",
});
let trial = 0;
const pair = () => {
  const start = 100000 + trial++ * 10000;
  return {
    surf: {
      ...run(),
      tap,
      startedAt: new Date(start).toISOString(),
      endedAt: new Date(start + 200).toISOString(),
    },
    chrome: {
      ...run(),
      wallMs: 400,
      report: structuredClone(chrome),
      startedAt: new Date(start + 1000).toISOString(),
      endedAt: new Date(start + 1400).toISOString(),
    },
  };
};

test("parses real reporter shapes, preserving nested identities and elapsed durations", () => {
  assert.equal(parseAuthTap(tap, expected).size, 46);
  assert.equal(parseAuthChrome(chrome, expected).size, 46);
  const result = compareAuthTrials(expected, [pair()]);
  assert.equal(result.wall.pairedMedianSpeedup, 2);
  assert.equal(result.cases[0].speedup, 2);
  assert.equal(result.testElapsedSum.surfMedianMs, 92);
});

test("accepts top-level specs shaped like the actual original deep-link Chrome control", () => {
  // Schema checked against the original Playwright 1.65 control report; all data
  // below is synthetic and contains no account IDs, URLs or captured stdout.
  const cases = expected.map((item, index) => ({
    ...item,
    file: "e2e/src/auth/loginReturnUrlStep.spec.ts",
    title_path: [`synthetic deep link ${index}`],
    id: `chromium:synthetic-${index}`,
  }));
  const report = {
    errors: [],
    suites: [
      {
        title: "src/auth/loginReturnUrlStep.spec.ts",
        file: "src/auth/loginReturnUrlStep.spec.ts",
        specs: cases.map((item, index) => ({
          title: item.title_path[0],
          file: "src/auth/loginReturnUrlStep.spec.ts",
          id: `synthetic-${index}`,
          ok: true,
          tests: [
            {
              projectName: "chromium",
              projectId: "chromium",
              expectedStatus: "passed",
              status: "expected",
              results: [{ status: "passed", duration: 4, retry: 0, errors: [] }],
            },
          ],
        })),
      },
    ],
  };
  assert.equal(parseAuthChrome(report, cases).size, 46);
  report.suites[0].specs[0].id = "forged";
  assert.throws(() => parseAuthChrome(report, cases), /collected ID/);
});

test("rejects failing, skipped, missing, duplicate and ambiguous TAP cases", () => {
  for (const invalid of [
    tap.replace("ok 1 - case 0", "not ok 1 - case 0"),
    tap.replace("ok 1 - case 0", "ok 1 - case 0 # SKIP"),
    tap.replace("case 0", "unexpected"),
    tap.replaceAll("case 1\n", "case 0\n"),
    tap.replace("# tests 46", "# tests 45"),
    tap.replace("duration_ms: 2", "duration_ms: NaN"),
  ]) {
    assert.throws(() => parseAuthTap(invalid, expected));
  }
  assert.throws(() => parseAuthTap(tap, [...expected.slice(1), expected[1]]));
});

test("rejects Chrome skips, retries, errors, wrong project and missing cases", () => {
  for (const mutate of [
    (report) => {
      report.errors.push({ message: "error" });
    },
    (report) => {
      report.suites[0].suites[0].specs.pop();
    },
    (report) => {
      report.suites[0].suites[0].specs[0].tests[0].results[0].status = "skipped";
    },
    (report) => {
      report.suites[0].suites[0].specs[0].tests[0].results.push({ status: "passed", duration: 4 });
    },
    (report) => {
      report.suites[0].suites[0].specs[0].tests[0].projectName = "other";
    },
    (report) => {
      report.suites[0].suites[0].specs[0].file = "src/auth/forged.spec.ts";
    },
    (report) => {
      report.suites[0].suites[0].specs[0].tests[0].results[0].retry = 1;
    },
  ]) {
    const report = structuredClone(chrome);
    mutate(report);
    assert.throws(() => parseAuthChrome(report, expected));
  }
});

test("rejects nonpassing, suspended, contended, unmatched or diagnostic process evidence", () => {
  for (const patch of [
    { exitCode: 1 },
    { signal: "SIGTERM" },
    { instrumented: true },
    { suspensionDetected: true },
    { clockGapMs: 1001 },
    { contended: true },
    { workers: 2 },
    { fixtureReset: "different" },
    { setupNotes: "" },
    { wallMs: NaN },
  ]) {
    const observation = pair();
    Object.assign(observation.surf, patch);
    assert.throws(() => compareAuthTrials(expected, [observation]));
  }
});

test("reports repeated medians and zero timer resolution without inventing a parity gate", () => {
  const first = pair();
  const second = pair();
  second.surf.wallMs = 400;
  second.surf.endedAt = new Date(Date.parse(second.surf.startedAt) + 400).toISOString();
  second.surf.tap = tap.replaceAll("duration_ms: 2", "duration_ms: 0");
  first.surf.tap = second.surf.tap;
  const result = compareAuthTrials(expected, [first, second]);
  assert.equal(result.wall.surfMedianMs, 300);
  assert.equal(result.wall.pairedMedianSpeedup, 1.5);
  assert.equal(result.cases[0].speedup, null);
  assert.equal(result.cases[0].resolutionLimited, true);
  assert.equal(Object.hasOwn(result, "parityPassed"), false);
});

test("rejects overlapping matched trials and clock evidence inconsistent with timestamps", () => {
  const observation = pair();
  observation.chrome.startedAt = observation.surf.startedAt;
  observation.chrome.endedAt = new Date(
    Date.parse(observation.chrome.startedAt) + 400,
  ).toISOString();
  assert.throws(() => compareAuthTrials(expected, [observation]), /overlap/);
  const gap = pair();
  gap.surf.endedAt = new Date(Date.parse(gap.surf.endedAt) + 5000).toISOString();
  assert.throws(() => compareAuthTrials(expected, [gap]), /clock discrepancy/);
});
