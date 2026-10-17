// Fresh installed-Chrome controls for native supports-conditioned layout. Never instruments page APIs.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const output = process.argv[2];
if (!output)
  throw new Error("Usage: node authored-css-selector-lexemes-chrome.mjs output-directory");
const directory = resolve(output);
const probes = ["css-selector-lexemes"];
const server = createServer((_request, response) => {
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><html><head></head><body></body></html>");
});
try {
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  for (const name of probes) {
    const destination = join(directory, name);
    mkdirSync(destination, { recursive: true });
    const probe = fileURLToPath(new URL(`./${name}-probe.js`, import.meta.url));
    const source = readFileSync(probe, "utf8");
    const config = join(destination, "config.json");
    writeFileSync(
      config,
      JSON.stringify({
        url: `http://127.0.0.1:${server.address().port}/`,
        waitMs: 25,
        disableExtensions: true,
        steps: [],
        probes: [probe],
      }),
    );
    const child = spawn(
      process.execPath,
      [fileURLToPath(new URL("./chrome-cdp-trace.mjs", import.meta.url)), config, destination],
      { stdio: "inherit" },
    );
    await new Promise((done, fail) => {
      child.once("error", fail);
      child.once("exit", (code) =>
        code === 0 ? done() : fail(new Error(`Chrome collector exited ${code}`)),
      );
    });
    const trace = JSON.parse(readFileSync(join(destination, "trace.json"), "utf8"));
    if (trace.errors.length || trace.probes.length !== 1 || trace.probes[0].error)
      throw new Error(`Invalid ${name} trace: ${JSON.stringify(trace.errors)}`);
    writeFileSync(
      join(destination, "fixture.json"),
      `${JSON.stringify({ browser: trace.browserVersion, v8: trace.browserV8Version, probeSha256: createHash("sha256").update(source).digest("hex"), result: trace.probes[0].result }, null, 2)}\n`,
    );
  }
} finally {
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
}
