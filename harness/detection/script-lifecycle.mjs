// Shared localhost async/defer script oracle; all traces use the existing differential harness.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
export function scriptLifecycleErrors(result) {
  const expected = [
    "inline:loading",
    "body:loading",
    "defer:interactive",
    "dcl:interactive",
    "async:interactive",
    "load:complete",
  ];
  const errors = [];
  if (JSON.stringify(result?.order) !== JSON.stringify(expected))
    errors.push("script/DCL/load ordering");
  if (result?.asyncReady !== true) errors.push("async script execution");
  if (
    JSON.stringify(result?.events) !==
    JSON.stringify([{ type: "load", trusted: true, event: true, target: true }])
  )
    errors.push("native script load event");
  return errors;
}

export async function captureScriptLifecycle({ binary, out }) {
  const fixture = readFileSync(join(here, "fixtures/script-lifecycle.html"));
  let releaseAsync;
  let gate = Promise.resolve();
  const server = createServer((request, response) => {
    if (request.url === "/") {
      gate = new Promise((done) => {
        releaseAsync = done;
      });
      response.setHeader("Content-Type", "text/html");
      response.end(fixture);
    } else if (request.url === "/ready") {
      releaseAsync?.();
      response.end("ready");
    } else if (request.url === "/async.js") {
      gate.then(() => {
        response.setHeader("Content-Type", "application/javascript");
        response.end("scriptOrder.push('async:'+document.readyState);window.asyncReady=true;");
      });
    } else if (request.url === "/defer.js") {
      response.setHeader("Content-Type", "application/javascript");
      response.end("scriptOrder.push('defer:'+document.readyState);");
    } else {
      response.writeHead(404);
      response.end();
    }
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const args = [
      join(here, "run.mjs"),
      "--live",
      "--url",
      `http://127.0.0.1:${server.address().port}/`,
      "--no-interaction",
      "--wait",
      "500",
      "--success-selector",
      "body[data-browser-fixture]",
      "--binary",
      binary,
      "--out",
      out,
    ];
    await new Promise((done, fail) => {
      const child = spawn(process.execPath, args, { stdio: "inherit" });
      child.on("error", fail);
      child.on("close", (code) =>
        code === 0 ? done() : fail(new Error(`Trace harness exited ${code}`)),
      );
    });
    const result = {};
    for (const pass of ["baseline", "interfaces"]) {
      const rows = ["chrome", "native"].map(
        (runtime) =>
          JSON.parse(readFileSync(join(out, `${runtime}-${pass}`, "trace.json"))).pages[0]
            .browserFixture,
      );
      result[pass] = {
        chrome: rows[0],
        native: rows[1],
        fixtureErrors: rows.map(scriptLifecycleErrors),
        differences: differences(rows[0], rows[1]),
      };
    }
    const record = {
      schemaVersion: 1,
      fixtureSha256: createHash("sha256").update(fixture).digest("hex"),
      collectorSha256: createHash("sha256")
        .update(readFileSync(fileURLToPath(import.meta.url)))
        .digest("hex"),
      passes: result,
    };
    writeFileSync(join(out, "script-comparison.json"), JSON.stringify(record, null, 2), {
      mode: 0o600,
    });
    return result;
  } finally {
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2),
    flag = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const out = resolve(flag("--out", join(here, "artifacts", `scripts-${Date.now()}`)));
  const result = await captureScriptLifecycle({
    out,
    binary: resolve(
      flag("--binary", join(here, "../../rust/target/debug/examples/detection_trace")),
    ),
  });
  console.log(JSON.stringify({ out, result }));
  if (
    Object.values(result).some(
      (row) => row.differences.length || row.fixtureErrors.some((errors) => errors.length),
    )
  )
    process.exitCode = 1;
}
