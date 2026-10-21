// Reusable localhost control. Actual timing is evidence, never an upper-bound assertion.
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { superviseChild } from "./child-supervision.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const hash = (name) =>
  createHash("sha256")
    .update(readFileSync(join(here, name)))
    .digest("hex");
export async function captureWindowTimerSemantics(outputArgument, signal) {
  if (typeof WebSocket === "undefined")
    throw new Error("This Chrome collector requires Node 22 WebSocket support");
  const output = resolve(outputArgument);
  const evidence = join(dirname(output), `${basename(output)}.capture`);
  if (existsSync(output) || existsSync(evidence))
    throw new Error("Preserve prior timer controls; choose a fresh output path");
  mkdirSync(evidence, { recursive: true, mode: 0o700 });
  const html = readFileSync(join(here, "window-timer-semantics.html"));
  const server = createServer((request, response) => {
    if (request.url !== "/") {
      response.writeHead(204).end();
      return;
    }
    response.writeHead(200, { "content-type": "text/html" });
    response.end(html);
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const configPath = join(evidence, "config.json");
    writeFileSync(
      configPath,
      JSON.stringify({
        url: `http://127.0.0.1:${server.address().port}/`,
        chromePath:
          process.env.TURBO_SURF_CHROME_PATH ||
          "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        chromeIncognito: true,
        interfaces: false,
        debugger: false,
        disableExtensions: true,
        steps: [],
        waitMs: 0,
        probes: [join(here, "window-timer-semantics-probe.js")],
      }),
      { mode: 0o600 },
    );
    for (const name of ["stdout", "stderr"])
      writeFileSync(join(evidence, `${name}.log`), "", { mode: 0o600 });
    const processResult = await superviseChild(
      process.execPath,
      [join(here, "chrome-cdp-trace.mjs"), configPath, join(evidence, "trace")],
      {
        timeoutMs: 45000,
        signal,
        onOutput: (name, bytes) => appendFileSync(join(evidence, `${name}.log`), bytes),
      },
    );
    writeFileSync(join(evidence, "process.json"), JSON.stringify(processResult, null, 2) + "\n", {
      mode: 0o600,
    });
    if (
      processResult.code !== 0 ||
      processResult.timedOut ||
      processResult.aborted ||
      processResult.error
    )
      throw new Error("Timer Chrome collection failed; evidence retained");
    const trace = JSON.parse(readFileSync(join(evidence, "trace/trace.json"), "utf8"));
    if (
      trace.captureComplete !== true ||
      !Array.isArray(trace.collectorErrors) ||
      trace.collectorErrors.length ||
      trace.errors.length
    )
      throw new Error("Timer Chrome collection was incomplete or reported an exception");
    if (trace.probes.length !== 1 || trace.probes[0].error || !trace.probes[0].result?.semantics)
      throw new Error("Timer probe did not produce its measured result");
    const control = {
      browser: trace.browserVersion,
      v8: trace.browserV8Version,
      sourceHashes: Object.fromEntries(
        [
          "window-timer-semantics-chrome.mjs",
          "window-timer-semantics-probe.js",
          "window-timer-semantics.html",
          "chrome-cdp-trace.mjs",
          "observation-window.mjs",
          "main-navigation.mjs",
          "collector-lifecycle.mjs",
        ].map((name) => [name, hash(name)]),
      ),
      errors: trace.errors,
      result: trace.probes[0].result,
      scope:
        "Genuine localhost task semantics and observed nesting delays. Timing is not an execution-time guarantee.",
    };
    writeFileSync(output, JSON.stringify(control, null, 2) + "\n", { flag: "wx", mode: 0o600 });
    return control;
  } finally {
    server.closeAllConnections();
    if (server.listening) await new Promise((done) => server.close(done));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error("Usage: node window-timer-semantics-chrome.mjs output.json");
  const abort = new AbortController();
  const stop = () => abort.abort(new Error("Timer control interrupted"));
  process.on("SIGTERM", stop);
  process.on("SIGINT", stop);
  try {
    await captureWindowTimerSemantics(process.argv[2], abort.signal);
  } finally {
    process.off("SIGTERM", stop);
    process.off("SIGINT", stop);
  }
}
