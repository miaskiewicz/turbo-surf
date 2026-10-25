// Capture the authored zoom/root-viewport/image-input geometry fixture using the
// independently installed Chrome. Standards probes run on a doctype document and
// quirks probes on the same markup without a doctype, each in its own capture.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const { values } = parseArgs({
  options: {
    runner: { type: "string" },
    out: { type: "string" },
    chrome: { type: "string" },
    probe: { type: "string", multiple: true },
  },
});
if (!values.runner || !values.out)
  throw Error("Pass --runner /absolute/chrome-cdp-trace.mjs --out /absolute/oracle.json");
// [probe, document path]; the path selects the served document mode.
const probes = values.probe
  ? values.probe.map((entry) => entry.split("@"))
  : [
      ["zoom-layout-probe.js", "/"],
      ["root-viewport-probe.js", "/"],
      ["root-viewport-probe.js", "/quirks"],
      ["image-input-size-probe.js", "/"],
    ];
const documents = {
  "/": "<!doctype html><html><head></head><body></body></html>",
  "/quirks": "<html><head></head><body></body></html>",
};
const directory = mkdtempSync(join(tmpdir(), "turbo-zoom-layout-oracle-"));
const server = createServer((request, response) => {
  response.setHeader("Content-Type", "text/html");
  response.end(documents[new URL(request.url, "http://x").pathname] ?? documents["/"]);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
let complete = false;
try {
  const results = {};
  let trace;
  for (const [index, [name, path]] of probes.entries()) {
    const config = join(directory, `config-${index}.json`);
    writeFileSync(
      config,
      JSON.stringify({
        url: `http://127.0.0.1:${server.address().port}${path}`,
        waitMs: 25,
        disableExtensions: true,
        foregroundFrames: true,
        steps: [],
        probes: [resolve(root, "harness/detection", name)],
        ...(values.chrome ? { chromePath: resolve(values.chrome) } : {}),
      }),
    );
    const output = join(directory, `capture-${index}`);
    const child = spawn(process.execPath, [resolve(values.runner), config, output], {
      stdio: "inherit",
    });
    const status = await new Promise((done) => child.once("exit", done));
    if (status !== 0) throw Error(`Chrome capture exited ${status}`);
    trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
    if (trace.errors.length || trace.probes.length !== 1 || trace.probes[0].error)
      throw Error(`Chrome control failed for ${name}; inspect preserved diagnostics`);
    results[path === "/" ? name : `${name}@${path}`] = trace.probes[0].result;
  }
  const oracle = {
    browser: trace.browserVersion,
    v8: trace.browserV8Version,
    recordedAt: new Date().toISOString(),
    fixtures: Object.fromEntries(
      [...new Set(probes.map(([name]) => name))].map((name) => [
        name,
        createHash("sha256")
          .update(readFileSync(resolve(root, "harness/detection", name)))
          .digest("hex"),
      ]),
    ),
    documents,
    effectiveExtensionFlags: trace.effectiveExtensionFlags,
    result: results,
  };
  writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
  console.log(JSON.stringify({ browser: oracle.browser, out: resolve(values.out) }));
  complete = true;
} finally {
  await new Promise((done) => server.close(done));
  if (complete) rmSync(directory, { recursive: true, force: true });
  else console.error(`Failed capture preserved at ${directory}`);
}
