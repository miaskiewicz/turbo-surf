// One-command Chrome re-version: detect → freeze → battery → diff → fixture +
// TRANSPORT-<major>.json → registry → turbo-surf parity → delta report.
//
//   node harness/detection/reversion-chrome.mjs --chrome "/Applications/Google Chrome.app"
//   node harness/detection/reversion-chrome.mjs --chrome <app> --check   # CI: changes nothing
//
// Safe to re-run: the frozen oracle is reused when its bundle hash still matches,
// the fixture for an already-measured build is re-measured and diffed, and the
// registry entry for a major is replaced in place. Exit codes: 0 done/current,
// 2 usage, 3 a delta needs a human (nothing written to the registry), 10 a newer
// Chrome stable exists (--check), 11 Chrome's identity drifted from its fixture,
// 12 turbo-surf drifted from the fixture (unexplained tells), 1 any other error.
// CHROME-REVERSION.md is the handbook.
import { execFile, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { collect, collectorHashes } from "./chrome-identity-battery.mjs";
import {
  diffParity,
  diffVersions,
  measuredRelease,
  representativeConnection,
  transportView,
  versionOf,
} from "./identity-diff.mjs";

const run = promisify(execFile);
const here = fileURLToPath(new URL(".", import.meta.url));
export const ROOT = resolve(here, "../..");
export const REGISTRY = join(ROOT, "rust/crates/turbo-surf-core/src/browser_releases.json");
export const FIXTURES = join(here, "fixtures");
export const KNOWN_GAPS = join(here, "chrome-identity-known-gaps.json");
export const ENGINE_STEP = join(here, "engine/reversion_v8.py");
export const EXIT = { ok: 0, usage: 2, human: 3, newer: 10, chromeDrift: 11, nativeDrift: 12 };
export const fixturePath = (major) => join(FIXTURES, `chrome-identity-${major}.json`);
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/** Registry JSON (the source build.rs compiles into browser::RELEASES). */
export const readRegistry = (path = REGISTRY) => readJson(path);

/** The default release and its fixture: what version-agnostic tests assert against. */
export function currentIdentity(registry = readRegistry()) {
  const release = registry.releases.find((entry) => entry.major === registry.defaultMajor);
  const path = fixturePath(registry.defaultMajor);
  return { release, fixturePath: path, fixture: existsSync(path) ? readJson(path) : null };
}

const versionParts = (version) => version.split(".").map(Number);
/** -1 / 0 / 1 for dotted Chrome versions. */
export function compareVersions(left, right) {
  const a = versionParts(left);
  const b = versionParts(right);
  for (let i = 0; i < 4; i++)
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) < (b[i] ?? 0) ? -1 : 1;
  return 0;
}

/** `current`, `new-build` or `new-major` for an installed Chrome vs the registry. */
export function releaseStatus(installed, registry) {
  const current = registry.releases.find((entry) => entry.major === registry.defaultMajor);
  const order = compareVersions(installed, current.fullVersion);
  if (order <= 0) return order === 0 ? "current" : "older";
  return Number(installed.split(".")[0]) > current.major ? "new-major" : "new-build";
}

/**
 * Insert or replace a release (newest first) and move the default to the newest
 * measured major. Returns a new registry object; the input is not mutated.
 */
export function applyRelease(registry, release, evidence) {
  const entry = { ...release, evidence };
  const releases = registry.releases.filter((existing) => existing.major !== release.major);
  releases.push(entry);
  releases.sort((a, b) => b.major - a.major);
  return {
    ...registry,
    defaultMajor: Math.max(registry.defaultMajor, release.major),
    releases,
  };
}

/** Stop conditions: release deltas the registry/transport cannot absorb as data. */
export function blockers(versionDiff, { transportBase } = {}) {
  const found = versionDiff.errors.map((message) => ({ reason: message }));
  for (const delta of versionDiff.deltas) {
    if (delta.action !== "human") continue;
    const where = delta.kind ?? delta.origin ?? "";
    const what = delta.header ?? delta.path ?? delta.type;
    let reason;
    if (delta.area === "transport")
      reason = `TLS/H2 changed at ${what} (${where}): the wreq-util Chrome${transportBase} recipe no longer matches; cut a transport recipe (rust/release-deps/transport) before re-versioning`;
    else if (delta.type === "header-missing" || delta.type === "header-extra")
      reason = `${delta.type === "header-extra" ? "new" : "removed"} header ${what} on ${where}: no registry field carries it; teach request_context/ClientHints, then re-run`;
    else if (delta.type === "request-missing" || delta.type === "request-new")
      reason = `battery request ${where} ${delta.type}: the collector or browser behaviour changed`;
    else if (delta.area === "js")
      reason = `JS identity ${what} changed (${JSON.stringify(delta.expected)} → ${JSON.stringify(delta.actual)}): navigator/userAgentData values are code in runtime.rs, not registry data`;
    else
      reason = `${delta.type} ${what} on ${where} changed (${JSON.stringify(delta.expected)} → ${JSON.stringify(delta.actual)})`;
    found.push({ reason, delta });
  }
  return found;
}

/**
 * Same-build re-measurement deltas that matter (environment noise is dropped).
 * A changed collector explains every delta, so it is reported as one entry.
 */
export function drift(saved, measured) {
  const changed = Object.keys({ ...saved.collector, ...measured.collector }).filter(
    (name) => saved.collector?.[name] !== measured.collector?.[name],
  );
  if (changed.length)
    return [
      {
        area: "collector",
        type: "collector-changed",
        path: changed.join(", "),
        action: "registry",
      },
    ];
  return diffVersions(saved, measured).deltas.filter((delta) => delta.action !== "info");
}

/** Resolve a Chrome .app bundle or executable to `{ app, executable, version }`. */
export async function detectChrome(path) {
  const target = resolve(path);
  if (target.endsWith(".app")) {
    const plist = join(target, "Contents/Info.plist");
    const field = async (key) =>
      (await run("/usr/bin/plutil", ["-extract", key, "raw", plist])).stdout.trim();
    const executable = join(target, "Contents/MacOS", await field("CFBundleExecutable"));
    return { app: target, executable, version: await field("CFBundleShortVersionString") };
  }
  const { stdout } = await run(target, ["--version"]);
  const version = /(\d+\.\d+\.\d+\.\d+)/.exec(stdout)?.[1];
  if (!version) throw new Error(`Cannot read a Chrome version from ${stdout.trim()}`);
  const app = /^(.*\.app)\/Contents\/MacOS\//.exec(target)?.[1] ?? null;
  return { app, executable: target, version };
}

/** Freeze (or reuse) the oracle bundle under .native-builds/oracles. */
export async function freezeOracle(chrome) {
  if (!chrome.app || process.platform !== "darwin") return { ...chrome, frozen: false };
  const { freezeChromeBundle, snapshotBundle } = await import("./freeze-chrome.mjs");
  const [major, , build, patch] = chrome.version.split(".");
  const destination = join(
    ROOT,
    `.native-builds/oracles/chrome${major}-${build}.${patch}`,
    basename(chrome.app),
  );
  const receiptPath = `${destination}.provenance.json`;
  const source = await snapshotBundle(chrome.app);
  if (existsSync(receiptPath)) {
    const receipt = readJson(receiptPath);
    if (!receipt.complete || receipt.snapshot?.sha256 !== source.sha256)
      throw new Error(
        `${relative(ROOT, destination)} exists but does not match ${chrome.app} (bundle ${source.sha256.slice(0, 12)}); move it aside to re-freeze`,
      );
    return {
      ...chrome,
      executable: receipt.executable,
      bundleSha256: source.sha256,
      frozen: "reused",
    };
  }
  const receipt = await freezeChromeBundle(chrome.app, destination);
  return {
    ...chrome,
    executable: receipt.executable,
    bundleSha256: receipt.snapshot.sha256,
    frozen: true,
  };
}

/** Build (or locate) the native identity_battery example. */
export async function nativeBinary(explicit) {
  if (explicit) return resolve(explicit);
  const manifest = join(ROOT, "rust/Cargo.toml");
  await new Promise((done, fail) => {
    const child = spawn(
      "cargo",
      [
        "build",
        "--manifest-path",
        manifest,
        "-p",
        "turbo-surf-mcp",
        "--example",
        "identity_battery",
      ],
      { stdio: ["ignore", "inherit", "inherit"] },
    );
    child.on("error", fail);
    child.on("close", (code) =>
      code === 0 ? done() : fail(new Error(`cargo build exited ${code}`)),
    );
  });
  return join(ROOT, "rust/target/debug/examples/identity_battery");
}

/** Optional V8 engine step owned by harness/detection/engine (--skip-engine skips). */
export async function engineStep(
  chromeVersion,
  { skip, mode = "--resolve", script = ENGINE_STEP } = {},
) {
  if (skip) return { status: "skipped" };
  if (!existsSync(script)) return { status: "absent", script: relative(ROOT, script) };
  try {
    const { stdout } = await run("python3", [script, "--chrome-version", chromeVersion, mode], {
      maxBuffer: 16 * 1024 * 1024,
      timeout: 600000,
    });
    let report;
    try {
      report = JSON.parse(stdout);
    } catch {
      report = { stdout: stdout.slice(-4000) };
    }
    return { status: "ran", mode, report };
  } catch (error) {
    return { status: "failed", mode, error: String(error.stderr || error.message).slice(-4000) };
  }
}

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

/** TRANSPORT-<major>.json: Chrome vs turbo-surf TLS/H2 + navigation wire, raw hellos kept. */
export function transportReport(chrome, native, parity, previous) {
  const { major } = versionOf(chrome);
  const raw = {};
  const sides = { chrome };
  if (native) sides.native = native;
  const views = {};
  const records = {};
  for (const [side, fixture] of Object.entries(sides)) {
    const ids = new Set(
      fixture.requests
        .filter((request) => (side === "chrome" ? true : request.tier === "nojs"))
        .map((request) => request.connection),
    );
    const connection = representativeConnection(fixture.connections, "A", "h2", ids);
    if (!connection) continue;
    const bytes = Buffer.from(connection.hello.rawBase64, "base64");
    const name = `fixtures/transport-${side === "chrome" ? "chrome" : "native"}${major}-client-hello.bin`;
    raw[side] = { fixture: name, sha256: sha256(bytes), bytes: bytes.length, data: bytes };
    views[side] = transportView(connection);
    const nav = fixture.requests.find(
      (request) => request.kind === "nav" && (side === "chrome" || request.tier === "nojs"),
    );
    records[side] = {
      akamai: connection.h2?.akamai,
      ja4: connection.hello.ja4,
      navHeaders: nav?.headers,
    };
  }
  return {
    report: {
      schemaVersion: 3,
      collectedAt: chrome.collectedAt,
      chromeVersion: chrome.browser.product,
      v8: chrome.browser.v8,
      targetChromeMajor: major,
      upstreamBaseTemplateMajor:
        native?.build?.transportBaseChromeMajor ?? previous?.upstreamBaseTemplateMajor,
      note: "Authored loopback TLS ClientHello, HTTP/2 frame and navigation-header controls from the identity battery (chrome-identity-battery.mjs). Raw records keep exact order, GREASE, randoms and key shares; normalized views drop intentional randomness. No remote traffic or cookie reuse; matching bounded controls is not full wire parity.",
      dependency: previous?.dependency,
      sources: collectorHashes(),
      nativeBuild: native?.build ?? null,
      rawRecords: Object.fromEntries(
        Object.entries(raw).map(([side, { data: _data, ...rest }]) => [side, rest]),
      ),
      tls: { chrome: views.chrome?.hello, native: views.native?.hello },
      fingerprints: records,
      http2: { chrome: views.chrome?.h2, native: views.native?.h2 },
      differences: parity
        ? parity.deltas
            .filter((delta) => delta.area === "transport")
            .map(({ path, expected, actual, class: kind }) => ({
              path,
              expected,
              actual,
              class: kind,
            }))
        : null,
      identityParity: parity ? parity.summary : null,
    },
    raw,
  };
}

async function formatJson(paths) {
  const biome = join(ROOT, "node_modules/.bin/biome");
  if (existsSync(biome)) await run(biome, ["format", "--write", ...paths]).catch(() => {});
}

function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function printReport(report) {
  const line = (text = "") => console.log(text);
  line(
    `Chrome ${report.chrome.version} (${report.status}); registry default ${report.registryDefault}`,
  );
  if (report.oracle) line(`oracle: ${report.oracle.frozen} ${report.oracle.executable}`);
  if (report.versionDiff) {
    const { from, to, deltas } = report.versionDiff;
    line(`\n${from.full} → ${to.full}: ${deltas.length} release deltas`);
    for (const delta of deltas)
      line(
        `  [${delta.action}] ${delta.area} ${delta.type} ${delta.kind ?? delta.origin ?? ""} ${delta.header ?? delta.path ?? ""}`.trimEnd(),
      );
  }
  if (report.chromeDrift?.length)
    line(`\nChrome drift vs its fixture: ${report.chromeDrift.length} deltas`);
  if (report.parity) {
    line(`\nturbo-surf parity: ${JSON.stringify(report.parity.summary)}`);
    for (const delta of report.parity.deltas.filter((entry) => entry.class === "tell"))
      line(
        `  [tell] ${delta.area} ${delta.type} ${delta.tier ?? ""} ${delta.kind ?? delta.origin ?? ""} ${delta.header ?? delta.path ?? ""}`.trimEnd(),
      );
  }
  if (report.engine)
    line(`\nengine step: ${report.engine.status}; ${JSON.stringify(report.engineIdentity)}`);
  if (report.blockers?.length) {
    line("\nSTOPPED — these deltas need a human (see CHROME-REVERSION.md, 'Handling deltas'):");
    for (const blocker of report.blockers) line(`  - ${blocker.reason}`);
  }
  for (const path of report.written ?? []) line(`wrote ${relative(ROOT, path)}`);
  if (report.reportPath) line(`report ${relative(ROOT, report.reportPath)}`);
}

export async function main(argv) {
  const args = [...argv];
  const has = (name) => args.includes(name);
  const flag = (name) => (has(name) ? args[args.indexOf(name) + 1] : undefined);
  if (!flag("--chrome")) {
    console.error(
      "Usage: node reversion-chrome.mjs --chrome <Chrome.app|executable> [--check] [--offline] [--native <identity_battery>] [--skip-native] [--skip-engine] [--engine-mode --resolve|--check] [--fixture-only]",
    );
    return EXIT.usage;
  }
  const check = has("--check");
  const registry = readRegistry();
  const report = { registryDefault: registry.defaultMajor, written: [] };
  report.chrome = await detectChrome(flag("--chrome"));
  report.status = releaseStatus(report.chrome.version, registry);
  const major = Number(report.chrome.version.split(".")[0]);
  const baseline = currentIdentity(registry).fixture;
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\..*/, "Z");
  report.reportPath = join(here, "artifacts", `reversion-${major}-${stamp}.json`);
  const finish = (code) => {
    report.exitCode = code;
    writeJson(report.reportPath, report);
    printReport(report);
    return code;
  };

  if (check) {
    // CI / scheduled mode: never writes fixtures, the registry or oracles.
    if (has("--offline"))
      return finish(["new-major", "new-build"].includes(report.status) ? EXIT.newer : EXIT.ok);
    const measured = await collect({ chrome: report.chrome.executable });
    const own = existsSync(fixturePath(major)) ? readJson(fixturePath(major)) : null;
    if (own && versionOf(own).full === report.chrome.version)
      report.chromeDrift = drift(own, measured);
    if (!has("--skip-native") && baseline) {
      const native = await collect({ native: await nativeBinary(flag("--native")) });
      report.parity = diffParity(baseline, native, { knownGaps: readJson(KNOWN_GAPS) });
    }
    if (["new-major", "new-build"].includes(report.status)) return finish(EXIT.newer);
    if (report.chromeDrift?.length) return finish(EXIT.chromeDrift);
    if (report.parity?.summary.tell) return finish(EXIT.nativeDrift);
    return finish(EXIT.ok);
  }

  report.oracle = await freezeOracle(report.chrome);
  const fixture = await collect({ chrome: report.oracle.executable });
  if (fixture.missing.length || fixture.errors.length)
    throw new Error(
      `battery incomplete: missing ${fixture.missing.join(", ")}; errors ${fixture.errors.join("; ")}`,
    );
  fixture.browser.bundleSha256 = report.oracle.bundleSha256 ?? null;
  if (versionOf(fixture).full !== report.chrome.version)
    throw new Error(
      `frozen executable reports ${versionOf(fixture).full}, not ${report.chrome.version}`,
    );
  const target = fixturePath(major);
  if (existsSync(target) && versionOf(readJson(target)).full === report.chrome.version)
    report.chromeDrift = drift(readJson(target), fixture);
  writeJson(target, fixture);
  report.written.push(target);

  const previousRelease = registry.releases.find((entry) => entry.major === registry.defaultMajor);
  if (baseline && versionOf(baseline).major !== major) {
    report.versionDiff = diffVersions(baseline, fixture);
    report.blockers = blockers(report.versionDiff, {
      transportBase: previousRelease.transportBaseMajor,
    });
  } else report.blockers = [];
  report.engine = await engineStep(report.chrome.version, {
    skip: has("--skip-engine"),
    mode: flag("--engine-mode") ?? "--resolve",
  });
  if (report.blockers.length) return finish(EXIT.human);
  if (has("--fixture-only")) return finish(EXIT.ok);

  let release;
  try {
    release = measuredRelease(fixture, previousRelease);
  } catch (error) {
    report.blockers.push({ reason: error.message });
    return finish(EXIT.human);
  }
  const updated = applyRelease(registry, release, {
    fixture: relative(ROOT, target),
    browser: fixture.browser.product,
    v8: fixture.browser.v8,
    collectedAt: fixture.collectedAt,
    bundleSha256: fixture.browser.bundleSha256,
  });
  writeJson(REGISTRY, updated);
  report.written.push(REGISTRY);

  let native = null;
  if (!has("--skip-native")) {
    native = await collect({ native: await nativeBinary(flag("--native")) });
    report.parity = diffParity(fixture, native, { knownGaps: readJson(KNOWN_GAPS) });
    report.engineIdentity = {
      chromeV8: fixture.browser.v8,
      nativeV8: native.build.v8Version,
      matches: String(native.build.v8Version).startsWith(
        fixture.browser.v8.split(".").slice(0, 2).join("."),
      ),
    };
  }
  const previousTransport = [registry.defaultMajor, major]
    .map((value) => join(here, `TRANSPORT-${value}.json`))
    .find((path) => existsSync(path));
  const transport = transportReport(
    fixture,
    native,
    report.parity,
    previousTransport && readJson(previousTransport),
  );
  for (const { fixture: name, data } of Object.values(transport.raw)) {
    writeFileSync(join(here, name), data);
    report.written.push(join(here, name));
  }
  const transportPath = join(here, `TRANSPORT-${major}.json`);
  writeJson(transportPath, transport.report);
  report.written.push(transportPath);
  await formatJson(report.written.filter((path) => path.endsWith(".json")));
  return finish(report.parity?.summary.tell ? EXIT.nativeDrift : EXIT.ok);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (error) => {
      console.error(error.stack || String(error));
      process.exit(1);
    },
  );
}
