// Localhost API differential; never visits a search engine or imports cookies.
import { execFileSync } from "node:child_process";
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
  openSync,
  closeSync,
} from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { superviseChild } from "./child-supervision.mjs";

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

// Member counts are measured from this pair, rather than carried forward from
// an older browser/binary. Descriptor differences remain separate from absence.
export function memberInventory(chrome, native) {
  return Object.fromEntries(
    [
      ...new Set([
        ...Object.keys(chrome.constructors || {}),
        ...Object.keys(native.constructors || {}),
      ]),
    ]
      .sort()
      .map((name) => {
        const expected = chrome.constructors?.[name]?.prototypeMembers;
        const actual = native.constructors?.[name]?.prototypeMembers;
        if (!expected || !actual) {
          return [
            name,
            { measured: false, chrome: expected !== undefined, native: actual !== undefined },
          ];
        }
        return [
          name,
          {
            measured: true,
            missing: Object.keys(expected)
              .filter((key) => !Object.hasOwn(actual, key))
              .sort(),
            extra: Object.keys(actual)
              .filter((key) => !Object.hasOwn(expected, key))
              .sort(),
            changed: Object.keys(expected)
              .filter((key) => Object.hasOwn(actual, key))
              .sort()
              .flatMap((key) => differences(expected[key], actual[key], key)),
          },
        ];
      }),
  );
}

export async function runApiCollector(
  command,
  args,
  out,
  { env = {}, signal, timeoutMs = 60000, killGraceMs = 5000 } = {},
) {
  const logs = {};
  try {
    for (const name of ["stdout", "stderr"])
      logs[name] = openSync(join(out, `${name}.log`), "w", 0o600);
    const {
      stdout: _stdout,
      stderr: _stderr,
      ...status
    } = await superviseChild(command, args, {
      signal,
      timeoutMs,
      killGraceMs,
      spawnOptions: { cwd: root, env: { ...process.env, ...env } },
      onOutput(name, bytes) {
        writeFileSync(logs[name], bytes);
      },
    });
    writeFileSync(join(out, "process.json"), JSON.stringify(status, null, 2), { mode: 0o600 });
    if (status.code !== 0 || status.signal || status.timedOut || status.aborted || status.error) {
      const error = new Error(`API collector failed: ${JSON.stringify(status)}`);
      error.processResult = status;
      throw error;
    }
    return status;
  } finally {
    for (const descriptor of Object.values(logs)) closeSync(descriptor);
  }
}

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
  signal,
}) {
  if (fixtureUrl !== undefined) fixtureUrl = localFixtureUrl(fixtureUrl);
  if (!existsSync(binary)) throw new Error(`Build the detection_trace example first: ${binary}`);
  if (existsSync(join(out, "comparison.json"))) throw new Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  const frozen = join(out, "native-executable");
  copyFileSync(binary, frozen);
  chmodSync(frozen, 0o700);
  const collectorSource = readFileSync(collector);
  const frozenCollector = join(out, "collector.js");
  writeFileSync(frozenCollector, collectorSource, { mode: 0o600 });
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
      probes: [frozenCollector],
      successSelector: "body",
    };
    const configPath = join(out, "config.json");
    writeFileSync(configPath, JSON.stringify(config, null, 2), { mode: 0o600 });
    const targets = ["chrome", "native"].map((name) => join(out, name));
    for (const target of targets) mkdirSync(target, { mode: 0o700 });
    const runs = await Promise.allSettled([
      runApiCollector(
        process.execPath,
        [join(here, "chrome-cdp-trace.mjs"), configPath, targets[0]],
        targets[0],
        { signal },
      ),
      runApiCollector(frozen, [configPath, targets[1]], targets[1], {
        signal,
        env: { TURBO_SURF_TZ: config.timezone },
      }),
    ]);
    for (const result of runs) if (result.status === "rejected") throw result.reason;
    const traces = targets.map((target) => JSON.parse(readFileSync(join(target, "trace.json"))));
    const probes = traces.map((trace) =>
      trace.probes.find((probe) => probe.path === frozenCollector),
    );
    for (const probe of probes)
      if (!probe || !Object.hasOwn(probe, "result")) throw new Error(JSON.stringify(probe));
    const comparison = {
      schemaVersion: 1,
      collectedAt: new Date().toISOString(),
      note: "Uninstrumented localhost API contract queries; no site detection conclusion.",
      build,
      chromeVersion: traces[0].browserVersion,
      // The product version alone does not identify the linked engine patch.
      chromeV8Version: traces[0].browserV8Version,
      binaryHash: hash(readFileSync(frozen)),
      collectorHash: hash(collectorSource),
      collector,
      collectorSnapshot: "collector.js",
      chrome: probes[0].result,
      native: probes[1].result,
      members: memberInventory(probes[0].result, probes[1].result),
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
    const controller = new AbortController();
    const handlers = new Map(
      ["SIGTERM", "SIGINT"].map((name) => [
        name,
        () => {
          process.exitCode = name === "SIGINT" ? 130 : 143;
          controller.abort(new Error(`API capture interrupted by ${name}`));
        },
      ]),
    );
    for (const [name, handler] of handlers) process.on(name, handler);
    try {
      const result = await captureApiShape({
        binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace"))),
        collector: resolve(flag("--probe", join(here, "api-shape.js"))),
        fixtureUrl: flag("--fixture-url", undefined),
        out,
        signal: controller.signal,
      });
      console.log(
        JSON.stringify({
          out,
          engine: result.build.engine,
          differences: result.differences.length,
        }),
      );
    } finally {
      for (const [name, handler] of handlers) process.off(name, handler);
    }
  }
}
