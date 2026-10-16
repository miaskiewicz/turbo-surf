// Publish only selected evidence fields; raw captures contain credentials and bodies.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

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

export function summarizeRun(directory) {
  const manifest = json(join(directory, "manifest.json"));
  if (sha(join(directory, "native-executable")) !== manifest.binaryHash)
    throw new Error("Frozen executable differs from the manifest");
  const runs = {};
  for (const run of manifest.runs) {
    if (!["chrome", "native"].includes(run.runtime) || run.pass === "probes") continue;
    if (!["baseline", "interfaces"].includes(run.pass)) throw new Error("Unknown capture pass");
    if (!Number.isInteger(run.repeat) || run.repeat < 0) throw new Error("Invalid repeat");
    const repeatDirectory = existsSync(join(directory, `repeat-${run.repeat}`))
      ? join(directory, `repeat-${run.repeat}`)
      : directory;
    const name = `${run.runtime}-${run.pass}`;
    const key = `${run.repeat}/${name}`;
    if (Object.hasOwn(runs, key)) throw new Error(`Duplicate capture ${key}`);
    const tracePath = join(repeatDirectory, name, "trace.json");
    const entry = {
      exitCode: run.code ?? null,
      timedOut: Boolean(run.timedOut),
      traceMissing: !existsSync(tracePath),
    };
    runs[key] = entry;
    if (entry.traceMissing) continue;
    const trace = json(tracePath);
    const config = json(
      join(
        repeatDirectory,
        run.pass === "baseline" ? "baseline-config.json" : "instrumented-config.json",
      ),
    );
    const networkPath = join(repeatDirectory, name, "network.jsonl");
    // JSON strings may contain U+2028: only an actual newline separates JSONL records.
    const rows = existsSync(networkPath)
      ? read(networkPath).split("\n").filter(Boolean).map(JSON.parse)
      : [
          ...(trace.requests || []).map((row) => ({ ...row, event: "request" })),
          ...(trace.responses || []).map((row) => ({ ...row, event: "response" })),
        ];
    const requests = rows.filter(
      (row) =>
        row.event === "request" && ["/save", "/search", "/sorry/index"].includes(pathname(row)),
    );
    const saves = rows.filter((row) => row.event === "response" && pathname(row) === "/save");
    const searchTimes = requests.filter((row) => pathname(row) === "/search").map(timestamp);
    const firstSearch =
      searchTimes.length && searchTimes.every(Number.isFinite) ? Math.min(...searchTimes) : null;
    Object.assign(entry, {
      collectorComplete: trace.captureComplete ?? null,
      recordedErrorCount: trace.errors?.length || 0,
      traceSha256: sha(tracePath),
      startOrigin: publicOrigin(config),
      formFallbackDisabled: config.disableFormFallback === true,
      formFallbackUsed: Boolean(trace.formFallback),
      navigationSource: ["native-form", "fallback", "none"].includes(trace.navigationSource)
        ? trace.navigationSource
        : null,
      consent: {
        requests: requests.filter((row) => pathname(row) === "/save").length,
        responseStatuses: saves.map((row) => row.status),
        responseBeforeFirstSearch:
          firstSearch !== null &&
          saves.length &&
          saves.every((row) => Number.isFinite(timestamp(row)))
            ? saves.some((row) => timestamp(row) < firstSearch)
            : null,
      },
      navigationRequests: requests.map((row) => {
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
      }),
      pages: (trace.pages || []).map((page) => ({
        phase: page.phase,
        path: publicPath(page),
        origin: publicOrigin(page),
        status: page.status,
        results: page.results,
        nativeNavigationCount: page.nativeNavigationCount,
        readyState: page.readyState,
        hydrationBudgetExceeded:
          typeof page.hydration?.budget_exceeded === "boolean"
            ? page.hydration.budget_exceeded
            : null,
        pendingNavigation: typeof page.nav === "string" ? page.nav !== page.url : null,
        cookieNames: (page.cookies || []).map((cookie) => cookie.name).sort(),
      })),
    });
  }
  const baselines = Object.entries(runs).filter(([key]) => key.endsWith("/native-baseline"));
  return {
    schemaVersion: 3,
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
      successful: baselines.filter(([, run]) => {
        const page = run.pages?.at(-1);
        return (
          run.exitCode === 0 &&
          !run.timedOut &&
          run.collectorComplete === true &&
          run.formFallbackDisabled &&
          !run.formFallbackUsed &&
          run.navigationSource === "native-form" &&
          run.startOrigin === "https://www.google.com" &&
          page &&
          (page.phase === "native-final" || /^navigation-\d+$/.test(page.phase)) &&
          page.path === "/search" &&
          page.origin === "https://www.google.com" &&
          page.status === 200 &&
          page.readyState === "complete" &&
          page.hydrationBudgetExceeded === false &&
          page.pendingNavigation === false &&
          Number.isInteger(page.results) &&
          page.results > 0
        );
      }).length,
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
