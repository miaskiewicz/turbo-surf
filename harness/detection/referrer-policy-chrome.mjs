import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] || join(here, "artifacts/referrer-policy"));
mkdirSync(out, { recursive: true });
let cross;
const server = () =>
  createServer((request, response) => {
    const url = new URL(request.url, "http://fixture.test");
    response.setHeader("Access-Control-Allow-Origin", "*");
    if (url.pathname === "/redirect") {
      response.writeHead(302, {
        Location: cross + "/echo",
        "Referrer-Policy": url.searchParams.get("policy"),
      });
      response.end();
      return;
    }
    if (url.pathname === "/echo") {
      response.setHeader("Content-Type", "application/json");
      response.end(JSON.stringify({ referer: request.headers.referer ?? null }));
      return;
    }
    response.setHeader("Content-Type", "text/html");
    response.setHeader("Referrer-Policy", "same-origin");
    response.end(
      `<!doctype html><body data-cross="${cross}">Authored referrer policy control</body>`,
    );
  });
const sourceServer = server(),
  targetServer = server();
const listen = (s) =>
  new Promise((resolve, reject) => {
    s.once("error", reject);
    s.listen(0, "127.0.0.1", resolve);
  });
try {
  await listen(sourceServer);
  await listen(targetServer);
  cross = `http://127.0.0.1:${targetServer.address().port}`;
  const probe = join(here, "referrer-policy-probe.js");
  const config = {
    url: `http://127.0.0.1:${sourceServer.address().port}/page?authored=value#fragment`,
    locale: "en-US",
    languages: ["en-US", "en"],
    viewport: { width: 1280, height: 800 },
    screen: { width: 1920, height: 1080 },
    outerWindow: { width: 1280, height: 888 },
    deviceScaleFactor: 2,
    timezone: "America/Los_Angeles",
    waitMs: 0,
    disableExtensions: true,
    steps: [],
    probes: [probe],
  };
  const configPath = join(out, "config.json");
  writeFileSync(configPath, JSON.stringify(config));
  const process = spawn(
    globalThis.process.execPath,
    [join(here, "chrome-cdp-trace.mjs"), configPath, out],
    { stdio: "inherit" },
  );
  await new Promise((resolve, reject) => {
    process.once("error", reject);
    process.once("exit", (code) =>
      code === 0 ? resolve() : reject(new Error(`Chrome exited ${code}`)),
    );
  });
  const trace = JSON.parse(readFileSync(join(out, "trace.json"), "utf8"));
  if (trace.errors.length || trace.probes[0].error)
    throw Error(JSON.stringify({ errors: trace.errors, probe: trace.probes[0] }));
  writeFileSync(
    join(out, "fixture.json"),
    JSON.stringify(
      {
        collectedAt: new Date().toISOString(),
        browserVersion: trace.browserVersion,
        browserV8Version: trace.browserV8Version,
        effectiveExtensionFlags: trace.effectiveExtensionFlags,
        collectorSha256: createHash("sha256").update(readFileSync(probe)).digest("hex"),
        result: trace.probes[0].result,
      },
      null,
      2,
    ),
  );
} finally {
  sourceServer.closeAllConnections();
  targetServer.closeAllConnections();
  await Promise.all([
    new Promise((r) => sourceServer.close(r)),
    new Promise((r) => targetServer.close(r)),
  ]);
}
