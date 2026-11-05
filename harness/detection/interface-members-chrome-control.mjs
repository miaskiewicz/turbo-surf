// Record the interface-member inventory control (turbo-surf#165) in an independently installed
// Chrome: interface-members-probe.js on a blank localhost page.
// node harness/detection/interface-members-chrome-control.mjs --runner $PWD/harness/detection/chrome-cdp-trace.mjs \
//   --chrome <chrome> --out harness/detection/fixtures/interface-members-chrome156.json
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const probe = "interface-members-probe.js";
const { values } = parseArgs({
  options: { runner: { type: "string" }, out: { type: "string" }, chrome: { type: "string" } },
});
if (!values.runner || !values.out) throw Error("Pass --runner and --out");

// The page the Rust test opens too.
const page = "<!doctype html><html><head></head><body></body></html>";
const server = createServer((_, response) => {
  response.setHeader("Content-Type", "text/html");
  response.end(page);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const directory = mkdtempSync(join(tmpdir(), "turbo-interface-members-oracle-"));
const hash = (name) =>
  createHash("sha256")
    .update(readFileSync(join(here, name)))
    .digest("hex");
let complete = false;
try {
  const config = join(directory, "config.json");
  writeFileSync(
    config,
    JSON.stringify({
      url: `http://127.0.0.1:${server.address().port}/`,
      viewport: { width: 800, height: 600 },
      deviceScaleFactor: 2,
      waitMs: 25,
      disableExtensions: true,
      steps: [],
      probes: [join(here, probe)],
      chromeArgs: ["--use-mock-keychain", "--password-store=basic"],
      ...(values.chrome ? { chromePath: resolve(values.chrome) } : {}),
    }),
  );
  const output = join(directory, "out");
  const child = spawn(process.execPath, [resolve(values.runner), config, output], {
    stdio: "inherit",
  });
  const status = await new Promise((done) => child.once("exit", done));
  if (status !== 0) throw Error(`Chrome capture exited ${status}`);
  const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
  const [row] = trace.probes;
  if (!row || row.error || !row.result) throw Error(`Chrome control failed; inspect ${output}`);
  const oracle = {
    browser: trace.browserVersion,
    v8: trace.browserV8Version,
    recordedAt: new Date().toISOString(),
    fixtures: { [probe]: hash(probe), page },
    result: row.result,
  };
  writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
  console.log(JSON.stringify({ browser: oracle.browser, out: resolve(values.out) }));
  complete = true;
} finally {
  await new Promise((done) => server.close(done));
  if (complete) rmSync(directory, { recursive: true, force: true });
}
