// Publish only selected evidence fields; raw captures contain credentials and bodies.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  summarizeNativeResponseTransport,
  summarizeResponseTransport,
} from "./chrome-network-metadata.mjs";

import { captureSummary, observationSummary } from "./compare.mjs";

const read = (path) => readFileSync(path, "utf8");
const json = (path) => JSON.parse(read(path));
const sha = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const pathname = (row) => new URL(row.url || "https://capture.invalid/").pathname;
const publicPath = (row) => {
  const path = pathname(row);
  return ["/", "/save", "/search", "/sorry", "/sorry/index"].includes(path) ? path : "<other>";
};
const publicOrigin = (row) =>
  new URL(row.url || "https://capture.invalid/").origin === "https://www.google.com"
    ? "https://www.google.com"
    : "<other>";
const timestamp = (row) => row.time_us ?? row.timeMs * 1000;
const cookieNames = (cookie) =>
  cookie
    .split(";")
    .map((part) => part.trim().split("=", 1)[0])
    .filter(Boolean)
    .sort();
const hydrationPhases = new Set([
  "document-loading",
  "init-script",
  "parser-and-deferred-scripts",
  "module-scripts",
  "document-interactive",
  "load-resources",
  "document-complete",
  "post-load-hydration",
  "post-load-quiescence",
  "complete",
]);
const hydrationProgress = (page) => {
  const progress = page.hydration?.progress;
  return {
    phase: hydrationPhases.has(progress?.phase) ? progress.phase : "unknown",
    completedPumps:
      Number.isSafeInteger(progress?.completed_pumps) && progress.completed_pumps >= 0
        ? progress.completed_pumps
        : null,
  };
};

const nonnegativeNumber = (value) =>
  typeof value === "number" &&
  Number.isFinite(value) &&
  value >= 0 &&
  value <= Number.MAX_SAFE_INTEGER
    ? value
    : null;
const count = (value) => (Number.isSafeInteger(value) && value >= 0 ? value : null);
const safeObservation = (trace) => {
  const observed = observationSummary(trace);
  return {
    evidence: observed.evidence,
    complete: observed.complete,
    policy: observed.policy === "fixed-deadline-before-enter" ? observed.policy : null,
    triggerStepIndex: count(observed.triggerStepIndex),
    requestedMs: count(observed.requestedMs),
    elapsedMs: nonnegativeNumber(observed.elapsedMs),
    overrunMs: nonnegativeNumber(observed.overrunMs),
    ...Object.fromEntries(
      ["pendingNavigation", "navigationFailed", "navigationWaitTimedOut", "navigationHopLimit"].map(
        (key) => [key, typeof observed[key] === "boolean" ? observed[key] : null],
      ),
    ),
  };
};

const terminationSignals = new Set([
  "SIGINT",
  "SIGTERM",
  "SIGKILL",
  "SIGABRT",
  "SIGSEGV",
  "SIGHUP",
  "SIGQUIT",
  "SIGBUS",
  "SIGILL",
  "SIGPIPE",
  "SIGTRAP",
  "SIGSYS",
  "SIGXCPU",
  "SIGXFSZ",
  "SIGUSR1",
  "SIGUSR2",
]);
const PROCESS_FIELDS = ["code", "timedOut", "aborted", "error", "signal"];
const nullOrString = (value) => value === null || typeof value === "string";
const validProcess = (run) =>
  (Number.isInteger(run.code) || run.code === null) &&
  typeof run.timedOut === "boolean" &&
  typeof run.aborted === "boolean" &&
  nullOrString(run.error) &&
  nullOrString(run.signal);
const cleanExit = (run) =>
  run.code === 0 &&
  run.timedOut === false &&
  run.aborted === false &&
  run.error === null &&
  run.signal === null;
const booleanOrNull = (value) => (typeof value === "boolean" ? value : null);
const errorPresent = (error) => {
  if (error === null) return false;
  return typeof error === "string" ? true : null;
};
const terminationSignal = (signal) => {
  if (typeof signal !== "string") return null;
  return terminationSignals.has(signal) ? signal : "other";
};
const processEvidence = (present, valid) => {
  if (!present) return "unmeasured";
  return valid ? "measured" : "invalid";
};
export function processSummary(run) {
  const present = PROCESS_FIELDS.every((field) => Object.hasOwn(run, field));
  const valid = validProcess(run);
  const evidence = processEvidence(present, valid);
  return {
    evidence,
    complete: evidence === "unmeasured" ? null : valid && cleanExit(run),
    exitCode: Number.isInteger(run.code) ? run.code : null,
    timedOut: booleanOrNull(run.timedOut),
    aborted: booleanOrNull(run.aborted),
    errorPresent: errorPresent(run.error),
    terminationSignal: terminationSignal(run.signal),
  };
}

const oneOf = (values, value, fallback) => (values.includes(value) ? value : fallback);

// Validates a manifest run, returning the directory its capture lives in.
function captureDirectory(directory, run) {
  if (!["baseline", "interfaces"].includes(run.pass)) throw new Error("Unknown capture pass");
  if (!Number.isInteger(run.repeat) || run.repeat < 0) throw new Error("Invalid repeat");
  return existsSync(join(directory, `repeat-${run.repeat}`))
    ? join(directory, `repeat-${run.repeat}`)
    : directory;
}

function networkRows(networkPath, trace) {
  // JSON strings may contain U+2028: only an actual newline separates JSONL records.
  if (existsSync(networkPath)) return read(networkPath).split("\n").filter(Boolean).map(JSON.parse);
  return [
    ...(trace.requests || []).map((row) => ({ ...row, event: "request" })),
    ...(trace.responses || []).map((row) => ({ ...row, event: "response" })),
  ];
}

function networkCoverage(runtime, trace) {
  if (runtime !== "chrome")
    return {
      source: "native-application-response",
      scope: "native-network-stack",
      packetCapture: false,
    };
  return {
    source: "cdp-network",
    scope: oneOf(
      ["attached-target-sessions", "main-page-session"],
      trace.networkCoverage?.scope,
      "unknown",
    ),
    enabledInBaseline: booleanOrNull(trace.networkCoverage?.enabledInBaseline),
    packetCapture: false,
  };
}

const finiteTimes = (rows) => rows.length && rows.every((row) => Number.isFinite(timestamp(row)));

function consentSummary(requests, saves) {
  const searchTimes = requests.filter((row) => pathname(row) === "/search").map(timestamp);
  const firstSearch =
    searchTimes.length && searchTimes.every(Number.isFinite) ? Math.min(...searchTimes) : null;
  return {
    requests: requests.filter((row) => pathname(row) === "/save").length,
    responseStatuses: saves.map((row) => row.status),
    responseBeforeFirstSearch:
      firstSearch !== null && finiteTimes(saves)
        ? saves.some((row) => timestamp(row) < firstSearch)
        : null,
  };
}

function navigationRequest(row) {
  const headers = Object.fromEntries(
    Object.entries(row.explicit_headers || row.headers || {}).map(([key, value]) => [
      key.toLowerCase(),
      value,
    ]),
  );
  return {
    path: pathname(row),
    method: row.method,
    context: Object.fromEntries(
      ["sec-fetch-user", "sec-fetch-site", "sec-fetch-mode", "sec-fetch-dest"]
        .filter((key) => Object.hasOwn(headers, key))
        .map((key) => [key, headers[key]]),
    ),
    cookieNames: cookieNames(headers.cookie || ""),
  };
}

const pageSummary = (page) => ({
  phase: page.phase,
  path: publicPath(page),
  origin: publicOrigin(page),
  status: page.status,
  results: page.results,
  nativeNavigationCount: page.nativeNavigationCount,
  readyState: page.readyState,
  hydrationBudgetExceeded: booleanOrNull(page.hydration?.budget_exceeded),
  hydrationProgress: hydrationProgress(page),
  pendingNavigation: typeof page.nav === "string" ? page.nav !== page.url : null,
  cookieNames: (page.cookies || []).map((cookie) => cookie.name).sort(),
});

function traceSummary(run, repeatDirectory, name, tracePath) {
  const trace = json(tracePath);
  const config = json(
    join(
      repeatDirectory,
      run.pass === "baseline" ? "baseline-config.json" : "instrumented-config.json",
    ),
  );
  const rows = networkRows(join(repeatDirectory, name, "network.jsonl"), trace);
  const requests = rows.filter(
    (row) =>
      row.event === "request" && ["/save", "/search", "/sorry/index"].includes(pathname(row)),
  );
  const saves = rows.filter((row) => row.event === "response" && pathname(row) === "/save");
  const capture = captureSummary(trace);
  return {
    collectorComplete: capture.complete,
    collectorCompletionSource: capture.completionSource,
    collectorErrorCount: capture.collectorErrorCount,
    observation: safeObservation(trace),
    windowTimerMode: oneOf(["real-time", "virtual"], trace.windowTimerMode, null),
    recordedErrorCount: trace.errors?.length || 0,
    responseTransport: (run.runtime === "chrome"
      ? summarizeResponseTransport
      : summarizeNativeResponseTransport)(rows.filter((row) => row.event === "response")),
    networkCoverage: networkCoverage(run.runtime, trace),
    traceSha256: sha(tracePath),
    startOrigin: publicOrigin(config),
    formFallbackDisabled: config.disableFormFallback === true,
    formFallbackUsed: Boolean(trace.formFallback),
    navigationSource: oneOf(
      ["native-form", "script", "form-fallback", "fallback", "none"],
      trace.navigationSource,
      null,
    ),
    consent: consentSummary(requests, saves),
    navigationRequests: requests.map(navigationRequest),
    pages: (trace.pages || []).map(pageSummary),
  };
}

function summarizeCapture(directory, run, runs) {
  const repeatDirectory = captureDirectory(directory, run);
  const name = `${run.runtime}-${run.pass}`;
  const key = `${run.repeat}/${name}`;
  if (Object.hasOwn(runs, key)) throw new Error(`Duplicate capture ${key}`);
  const tracePath = join(repeatDirectory, name, "trace.json");
  const process = processSummary(run);
  const entry = {
    exitCode: process.exitCode,
    timedOut: process.timedOut,
    process,
    traceMissing: !existsSync(tracePath),
  };
  runs[key] = entry;
  if (!entry.traceMissing)
    Object.assign(entry, traceSummary(run, repeatDirectory, name, tracePath));
}

const nativeRunSettled = (run) =>
  run.process.complete === true &&
  run.collectorComplete === true &&
  run.observation.complete === true &&
  run.windowTimerMode === "real-time" &&
  run.formFallbackDisabled &&
  !run.formFallbackUsed &&
  run.navigationSource === "native-form" &&
  run.startOrigin === "https://www.google.com";
const searchPageReached = (page) =>
  page &&
  (page.phase === "native-final" || /^navigation-\d+$/.test(page.phase)) &&
  page.path === "/search" &&
  page.origin === "https://www.google.com" &&
  page.status === 200 &&
  page.readyState === "complete" &&
  page.hydrationBudgetExceeded === false &&
  page.pendingNavigation === false &&
  Number.isInteger(page.results) &&
  page.results > 0;
const successfulBaseline = (run) => nativeRunSettled(run) && searchPageReached(run.pages?.at(-1));

export function summarizeRun(directory) {
  const manifest = json(join(directory, "manifest.json"));
  if (sha(join(directory, "native-executable")) !== manifest.binaryHash)
    throw new Error("Frozen executable differs from the manifest");
  const runs = {};
  for (const run of manifest.runs) {
    if (!["chrome", "native"].includes(run.runtime) || run.pass === "probes") continue;
    summarizeCapture(directory, run, runs);
  }
  const baselines = Object.entries(runs).filter(([key]) => key.endsWith("/native-baseline"));
  return {
    schemaVersion: 6,
    collectedAt: manifest.createdAt,
    baseCommit: manifest.commit,
    dirtyDiffHash: manifest.dirtyDiffHash,
    binaryHash: manifest.binaryHash,
    build: manifest.nativeBuildInfo,
    harnessHashes: manifest.harnessHashes,
    repeats: Math.max(-1, ...manifest.runs.map((run) => run.repeat)) + 1,
    errorCountScope:
      "Counts trace.errors; process stdout/stderr and debugger diagnostics remain separate.",
    nativeBaseline: {
      captured: baselines.length,
      processMeasured: baselines.filter(([, run]) => run.process.evidence === "measured").length,
      observationMeasured: baselines.filter(([, run]) => run.observation?.evidence === "measured")
        .length,
      successful: baselines.filter(([, run]) => successfulBaseline(run)).length,
    },
    runs,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [directory, output] = process.argv.slice(2);
  if (!directory || !output)
    throw new Error("Usage: summarize-run.mjs capture-directory output.json");
  const summary = summarizeRun(resolve(directory));
  writeFileSync(output, `${JSON.stringify(summary, null, 2)}\n`, { mode: 0o600 });
  console.log(JSON.stringify(summary.nativeBaseline));
}
