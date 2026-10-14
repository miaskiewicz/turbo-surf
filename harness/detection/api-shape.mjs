// Localhost API differential; never visits a search engine or imports cookies.
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { isIP } from "node:net";
import { createHash } from "node:crypto";
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  existsSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = resolve(here, "../..");
const absent = { absent: true };

export function differences(chrome, native, path = "") {
  if (chrome === absent || native === absent) return [{ path, chrome, native }];
  if (JSON.stringify(chrome) === JSON.stringify(native)) return [];
  if (
    chrome &&
    native &&
    typeof chrome === "object" &&
    typeof native === "object" &&
    !Array.isArray(chrome) &&
    !Array.isArray(native)
  ) {
    return [...new Set([...Object.keys(chrome), ...Object.keys(native)])]
      .sort()
      .flatMap((key) =>
        differences(
          Object.hasOwn(chrome, key) ? chrome[key] : absent,
          Object.hasOwn(native, key) ? native[key] : absent,
          path ? `${path}.${key}` : key,
        ),
      );
  }
  return [{ path, chrome, native }];
}

const run = (command, args, out, env = {}) =>
  new Promise((done, fail) => {
    const child = spawn(command, args, {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    const timeout = setTimeout(() => child.kill("SIGKILL"), 60_000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timeout);
      writeFileSync(join(out, "stdout.log"), stdout, { mode: 0o600 });
      writeFileSync(join(out, "stderr.log"), stderr, { mode: 0o600 });
      code === 0 ? done() : fail(new Error(`${command} failed (${code}): ${stderr}`));
    });
  });

export function localFixtureUrl(input) {
  const url = new URL(input);
  const host = url.hostname;
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    !(host === "localhost" || host === "[::1]" || (isIP(host) === 4 && host.startsWith("127.")))
  ) {
    throw new TypeError(
      "API differential fixtures must use a loopback HTTP(S) URL without credentials",
    );
  }
  return url.href;
}

export async function captureApiShape({
  binary,
  out,
  collector = join(here, "api-shape.js"),
  fixtureUrl,
}) {
  if (fixtureUrl !== undefined) fixtureUrl = localFixtureUrl(fixtureUrl);
  if (!existsSync(binary)) throw new Error(`Build the detection_trace example first: ${binary}`);
  if (existsSync(join(out, "comparison.json"))) throw new Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  const frozen = join(out, "native-executable");
  copyFileSync(binary, frozen);
  chmodSync(frozen, 0o700);
  const build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  const hash = (value) => createHash("sha256").update(value).digest("hex");
  const server = fixtureUrl
    ? null
    : createServer((_req, response) => {
        response.setHeader("Content-Type", "text/html");
        response.end("<!doctype html><html><head></head><body></body></html>");
      });
  try {
    if (server) {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
    }
    const config = {
      url: fixtureUrl || `http://127.0.0.1:${server.address().port}/`,
      viewport: { width: 1280, height: 800 },
      screen: { width: 1920, height: 1080 },
      outerWindow: { width: 1280, height: 888 },
      deviceScaleFactor: 2,
      languages: ["en-US", "en"],
      locale: "en-US",
      timezone: "America/Los_Angeles",
      waitMs: 0,
      interfaces: false,
      disableExtensions: true,
      steps: [],
      probes: [collector],
      successSelector: "body",
    };
    const configPath = join(out, "config.json");
    writeFileSync(configPath, JSON.stringify(config, null, 2), { mode: 0o600 });
    const targets = ["chrome", "native"].map((name) => join(out, name));
    for (const target of targets) mkdirSync(target, { mode: 0o700 });
    const runs = await Promise.allSettled([
      run(process.execPath, [join(here, "chrome-trace.mjs"), configPath, targets[0]], targets[0]),
      run(frozen, [configPath, targets[1]], targets[1], { TURBO_SURF_TZ: config.timezone }),
    ]);
    for (const result of runs) if (result.status === "rejected") throw result.reason;
    const traces = targets.map((target) => JSON.parse(readFileSync(join(target, "trace.json"))));
    const probes = traces.map((trace) => trace.probes.find((probe) => probe.path === collector));
    for (const probe of probes)
      if (!probe || !Object.hasOwn(probe, "result")) throw new Error(JSON.stringify(probe));
    const comparison = {
      schemaVersion: 1,
      collectedAt: new Date().toISOString(),
      note: "Uninstrumented localhost API contract queries; no site detection conclusion.",
      build,
      chromeVersion: traces[0].browserVersion,
      binaryHash: hash(readFileSync(frozen)),
      collectorHash: hash(readFileSync(collector)),
      collector,
      chrome: probes[0].result,
      native: probes[1].result,
      differences: differences(probes[0].result, probes[1].result),
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(comparison, null, 2), {
      mode: 0o600,
    });
    return comparison;
  } finally {
    if (server) await new Promise((done) => server.close(done));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name, fallback) => {
    const index = args.indexOf(name);
    return index < 0 ? fallback : args[index + 1];
  };
  if (!args.includes("--run")) {
    console.log(
      "Use --run for the localhost Chrome/native API audit; no Google requests are made.",
    );
  } else {
    const out = resolve(flag("--out", join(here, "artifacts", `api-${Date.now()}`)));
    const result = await captureApiShape({
      binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace"))),
      collector: resolve(flag("--probe", join(here, "api-shape.js"))),
      fixtureUrl: flag("--fixture-url", undefined),
      out,
    });
    console.log(
      JSON.stringify({ out, engine: result.build.engine, differences: result.differences.length }),
    );
  }
}
