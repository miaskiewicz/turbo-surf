// Rebuild the installed-Chrome stylesheet admission control from its Rust fixture.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const fixtureSource = "rust/crates/turbo-surf-render/tests/resource_scan_order.rs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const collectorSource = "harness/detection/chrome-cdp-trace.mjs";
const generatorSource = "harness/detection/resource-scan-order-chrome.mjs";
export const sha256 = (value) => createHash("sha256").update(value).digest("hex");
export function extractResourceFixture(source) {
  const match = source.match(/let html = r#"([\s\S]*?)"#;/);
  if (!match) throw new Error("Resource-order Rust fixture is missing its HTML raw string");
  return match[1];
}

async function generate(output) {
  const html = extractResourceFixture(readFileSync(join(root, fixtureSource), "utf8"));
  const temporary = mkdtempSync(join(tmpdir(), "turbo-resource-order-"));
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    response.setHeader("Content-Type", path === "/" ? "text/html" : "text/css");
    response.end(path === "/" ? html : "sentinel {color:blue}");
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const config = {
      url: `http://127.0.0.1:${server.address().port}/`,
      interfaces: false,
      debugger: false,
      steps: [],
      probes: [],
      waitMs: 50,
      viewport: { width: 1280, height: 800 },
      outerWindow: { width: 1280, height: 888 },
      screen: { width: 1920, height: 1080 },
      deviceScaleFactor: 2,
      languages: ["en-US", "en"],
      locale: "en-US",
      timezone: "America/Los_Angeles",
    };
    const configPath = join(temporary, "config.json");
    writeFileSync(configPath, JSON.stringify(config), { mode: 0o600 });
    const child = spawn(
      process.execPath,
      [join(root, collectorSource), configPath, join(temporary, "capture")],
      { stdio: ["ignore", "pipe", "pipe"] },
    );
    let log = "";
    for (const stream of [child.stdout, child.stderr])
      stream.on("data", (bytes) => {
        log = (log + bytes).slice(-1024 * 1024);
      });
    const code = await new Promise((done, fail) => {
      child.once("error", fail);
      child.once("close", done);
    });
    if (code !== 0) throw new Error(`Chrome collector exited ${code}: ${log}`);
    const trace = JSON.parse(readFileSync(join(temporary, "capture/trace.json"), "utf8"));
    const unexpected = (type) =>
      trace.requests
        .filter((row) => row.type === type && new URL(row.url).pathname.startsWith("/unexpected"))
        .map((row) => new URL(row.url).pathname);
    const control = {
      browser: trace.browserVersion,
      v8: trace.browserV8Version,
      scope:
        "Parser stylesheet order and script exclusion; SVG script execution is outside this control.",
      fixtureSource,
      fixtureSha256: sha256(html),
      collectorSha256: sha256(readFileSync(join(root, collectorSource))),
      generatorSha256: sha256(readFileSync(join(root, generatorSource))),
      errors: trace.errors,
      result: trace.pages[0]?.browserFixture ?? null,
      unexpectedStylesheetRequests: unexpected("stylesheet"),
      scriptFetchesOutsideControl: unexpected("script"),
    };
    if (control.errors.length || !control.result) throw new Error(JSON.stringify(control));
    writeFileSync(resolve(output), JSON.stringify(control, null, 2) + "\n", { mode: 0o600 });
  } finally {
    server.closeAllConnections();
    if (server.listening) await new Promise((done) => server.close(done));
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error("Usage: node resource-scan-order-chrome.mjs output.json");
  await generate(process.argv[2]);
}
