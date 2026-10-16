import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { resolve } from "node:path";
const root = resolve(import.meta.dirname, "..");
const out = resolve(process.argv[2]);
mkdirSync(out, { recursive: true });
const probe = resolve(root, process.argv[3] || "harness/detection/live-text-geometry-probe.js");
const runner = resolve(root, "harness/detection/chrome-cdp-trace.mjs");
const server = createServer((_, response) =>
  response.end("<!doctype html><html><head></head><body></body></html>"),
);
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const config = resolve(out, "config.json");
writeFileSync(
  config,
  JSON.stringify({
    url: `http://127.0.0.1:${server.address().port}/`,
    waitMs: 20,
    disableExtensions: true,
    steps: [],
    probes: [probe],
  }),
);
try {
  const child = spawn(process.execPath, [runner, config, resolve(out, "capture")], {
    stdio: "inherit",
  });
  const status = await new Promise((done) => child.once("exit", done));
  if (status !== 0) throw Error("Independent Chrome capture failed");
  const trace = JSON.parse(readFileSync(resolve(out, "capture/trace.json")));
  if (trace.errors.length || trace.probes[0]?.error || !trace.probes[0]?.result)
    throw Error("Independent Chrome probe failed");
  const sha = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
  const control = {
    browser: trace.browserVersion,
    v8: trace.browserV8Version,
    probeSha256: sha(probe),
    runnerSha256: sha(runner),
    result: trace.probes[0].result,
  };
  writeFileSync(
    resolve(
      root,
      process.argv[4] || "harness/detection/fixtures/live-text-geometry-chrome155.json",
    ),
    JSON.stringify(control, null, 2) + "\n",
  );
  console.log(JSON.stringify({ browser: control.browser, v8: control.v8 }));
} finally {
  await new Promise((done) => server.close(done));
}
