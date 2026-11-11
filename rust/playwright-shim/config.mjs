// Minimal Playwright-config interpreter for the turbo-surf shim.
// Reads the subset of playwright.config.{ts,mts,js,mjs,cjs} the shim can honor:
// `testDir` / `testMatch` / `testIgnore` for file routing, `projects` (each one
// is run as a separate sweep with its own `use` merged over the top-level one),
// `reporter` (list/spec/tap/html), and `webServer` (spawn + poll url + teardown).
// Callers run it through `bin/turbo-surf-test.mjs`.

import { spawn } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { validateArtifactOptions } from "./artifact-options.mjs";

const DEFAULT_CONFIG_NAMES = [
  "playwright.config.ts",
  "playwright.config.mts",
  "playwright.config.mjs",
  "playwright.config.js",
  "playwright.config.cjs",
];

export function findConfig(cwd = process.cwd()) {
  for (const n of DEFAULT_CONFIG_NAMES) {
    const p = resolve(cwd, n);
    if (existsSync(p)) return p;
  }
  return null;
}

export async function loadConfig(configPath) {
  const full = resolve(configPath);
  const mod = await import(pathToFileURL(full).href);
  const raw = mod.default ?? mod.config ?? mod;
  return normalizeConfig(raw, dirname(full));
}

const DEFAULT_TEST_MATCH = ["**/*.@(spec|test).@(ts|mts|js|mjs|cjs)"];
const DEFAULT_TEST_IGNORE = ["**/node_modules/**"];

export function normalizeConfig(raw, rootDir) {
  const r = raw ?? {};
  const testDir = resolve(rootDir, r.testDir ?? ".");
  const testMatch = toArray(r.testMatch) ?? DEFAULT_TEST_MATCH;
  const testIgnore = toArray(r.testIgnore) ?? DEFAULT_TEST_IGNORE;
  const reporter = normalizeReporter(r.reporter);
  const webServer = normalizeWebServer(r.webServer, rootDir);
  const use = r.use ?? {};
  const rawProjects = Array.isArray(r.projects) ? r.projects : [];
  const projects = rawProjects.length
    ? rawProjects.map((p) => ({
        name: String(p.name ?? "default"),
        dependencies: p.dependencies ?? [],
        grep: toArray(p.grep ?? r.grep),
        grepInvert: toArray(p.grepInvert ?? r.grepInvert),
        use: { ...use, ...p.use },
        testDir: p.testDir ? resolve(rootDir, p.testDir) : testDir,
        testMatch: toArray(p.testMatch) ?? testMatch,
        testIgnore: toArray(p.testIgnore) ?? testIgnore,
      }))
    : [
        {
          name: "default",
          use,
          testDir,
          testMatch,
          testIgnore,
          grep: toArray(r.grep),
          grepInvert: toArray(r.grepInvert),
        },
      ];
  return { rootDir, testDir, testMatch, testIgnore, reporter, webServer, use, projects };
}

function toArray(v) {
  if (v === null || v === undefined) return null;
  return Array.isArray(v) ? v.slice() : [v];
}

function normalizeReporter(r) {
  if (!r) return [["list"]];
  if (typeof r === "string") return [[r]];
  if (Array.isArray(r)) {
    return r.map((e) => (typeof e === "string" ? [e] : Array.isArray(e) ? e : [String(e)]));
  }
  return [[String(r)]];
}

function normalizeWebServer(w, rootDir) {
  if (!w) return [];
  return (Array.isArray(w) ? w : [w]).map((s) => ({
    command: s.command,
    url: s.url,
    port: s.port,
    cwd: resolve(rootDir, s.cwd ?? "."),
    env: s.env ?? {},
    timeout: s.timeout ?? 60_000,
    reuseExistingServer: s.reuseExistingServer ?? false,
    gracefulShutdown: s.gracefulShutdown ?? { signal: "SIGTERM", timeout: 5000 },
  }));
}

// Minimal POSIX-glob → RegExp. Handles **, *, ?, {a,b}, @(a|b|c) / +(…) / ?(…) /
// *(…) extglob (collapsed to a plain alternation — behavior matches Playwright's
// `testMatch` cases for the one-level patterns the config uses in practice).
export function globToRegex(glob) {
  let i = 0;
  let re = "";
  while (i < glob.length) {
    const c = glob[i];
    // extglob `@(a|b)` / `+(…)` / `*(…)` / `?(…)` → one alternation group.
    if ("@+*?".includes(c) && glob[i + 1] === "(") {
      const end = glob.indexOf(")", i + 2);
      if (end > i) {
        const inner = glob.slice(i + 2, end);
        re += `(?:${inner.split("|").map(escapeLiteral).join("|")})`;
        i = end + 1;
        continue;
      }
    }
    // brace expansion `{a,b}`.
    if (c === "{") {
      const end = glob.indexOf("}", i + 1);
      if (end > i) {
        const inner = glob.slice(i + 1, end);
        re += `(?:${inner.split(",").map(escapeLiteral).join("|")})`;
        i = end + 1;
        continue;
      }
    }
    if (c === "*") {
      if (glob[i + 1] === "*") {
        // `**` matches across path separators (including zero dirs when followed by `/`).
        re += ".*";
        i += 2;
        if (glob[i] === "/") i++;
        continue;
      }
      re += "[^/]*";
      i++;
      continue;
    }
    if (c === "?") {
      re += "[^/]";
      i++;
      continue;
    }
    re += "/.+^$|()[]\\".includes(c) ? `\\${c}` : c;
    i++;
  }
  return new RegExp(`^${re}$`);
}

function escapeLiteral(s) {
  return s.replace(/[.+^$|()[\]\\]/g, (c) => `\\${c}`);
}

export function matchAny(path, globs, absolutePath = path) {
  return globs.some((pattern) => {
    if (!(pattern instanceof RegExp)) return globToRegex(pattern).test(path);
    pattern.lastIndex = 0;
    const matched = pattern.test(absolutePath);
    pattern.lastIndex = 0;
    return matched;
  });
}

export function discoverTests(project) {
  const files = [];
  if (!existsSync(project.testDir)) return files;
  const walk = (dir) => {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) {
        walk(p);
      } else if (ent.isFile()) {
        const rel = relative(project.testDir, p).split(sep).join("/");
        if (matchAny(rel, project.testIgnore, p)) continue;
        if (matchAny(rel, project.testMatch, p)) files.push(p);
      }
    }
  };
  walk(project.testDir);
  files.sort();
  return files;
}

// A webServer entry boots an app under test. We spawn through the user's shell
// (Playwright does the same), poll `url` (or `http://127.0.0.1:${port}`) with
// cheap HEADless `fetch`es until it responds, and tear it down on exit.
export async function startWebServers(servers, log) {
  const handles = [];
  try {
    for (const s of servers) handles.push(await startWebServer(s, log));
    return handles;
  } catch (e) {
    await stopWebServers(handles);
    throw e;
  }
}

async function startWebServer(s, log) {
  if (s.reuseExistingServer && (await probe(s))) {
    log?.(`already up at ${probeURL(s)} — reusing`);
    return { child: null, server: s };
  }
  const child = spawn(s.command, {
    shell: true,
    detached: process.platform !== "win32",
    cwd: s.cwd,
    env: { ...process.env, ...s.env },
    stdio: "inherit",
  });
  const deadline = Date.now() + s.timeout;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`webServer exited early (code ${child.exitCode})`);
    if (await probe(s)) {
      log?.(`ready at ${probeURL(s)}`);
      return { child, server: s };
    }
    await delay(200);
  }
  await stopWebServers([{ child, server: s }]);
  throw new Error(`webServer timed out after ${s.timeout}ms waiting for ${probeURL(s)}`);
}

function probeURL(s) {
  return s.url ?? (s.port === undefined || s.port === null ? null : `http://127.0.0.1:${s.port}`);
}

async function probe(s) {
  const url = probeURL(s);
  if (!url) return false;
  try {
    const parsed = new URL(url);
    const status = await probeStatus(parsed);
    if (status === 404 && parsed.pathname === "/") {
      parsed.pathname = "/index.html";
      return readyStatus(await probeStatus(parsed));
    }
    return readyStatus(status);
  } catch {
    return false;
  }
}

async function probeStatus(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(2000) });
  await response.body?.cancel();
  return response.status;
}

function readyStatus(status) {
  return status >= 200 && status <= 403;
}

function delay(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function ownedServerAlive(child) {
  if (process.platform === "win32") return child.exitCode === null && child.signalCode === null;
  try {
    process.kill(-child.pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function signalOwnedServer(child, signal) {
  if (process.platform === "win32") {
    const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    await new Promise((resolve) => {
      killer.once("exit", resolve);
      killer.once("error", resolve);
    });
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

export async function stopWebServers(handles) {
  for (const h of handles) {
    if (!h?.child) continue;
    const sig = h.server.gracefulShutdown?.signal ?? "SIGTERM";
    const tmo = h.server.gracefulShutdown?.timeout ?? 5000;
    await signalOwnedServer(h.child, sig);
    const deadline = Date.now() + tmo;
    while (ownedServerAlive(h.child) && Date.now() < deadline) await delay(50);
    if (ownedServerAlive(h.child)) await signalOwnedServer(h.child, "SIGKILL");
  }
}

export function orderProjects(projects, selectedNames) {
  const byName = new Map(projects.map((project) => [project.name, project]));
  const ordered = [];
  const visiting = new Set();
  const visited = new Set();
  const visit = (project) => {
    if (visited.has(project.name)) return;
    if (visiting.has(project.name)) throw new Error(`Project dependency cycle at ${project.name}`);
    visiting.add(project.name);
    for (const name of project.dependencies ?? []) {
      const dependency = byName.get(name);
      if (!dependency) throw new Error(`Unknown dependency ${name} for project ${project.name}`);
      visit(dependency);
    }
    visiting.delete(project.name);
    visited.add(project.name);
    ordered.push(project);
  };
  for (const project of projects) {
    if (!selectedNames || selectedNames.includes(project.name)) visit(project);
  }
  return ordered;
}

export function testRunnerFlags(project, version = process.versions.node) {
  validateArtifactOptions(project.use);
  const [major, minor] = version.split(".").map(Number);
  const forceExit = major >= 22 || (major === 20 && minor >= 14);
  if (!forceExit) {
    throw new Error(
      `turbo-surf-test requires Node.js 20.14+ or 22+ for completed-test worker shutdown; current ${version}`,
    );
  }
  if (project.grepInvert?.length && !(major > 22 || (major === 22 && minor >= 1))) {
    throw new Error(`turbo-surf-test: grepInvert requires Node.js 22.1+; current ${version}`);
  }
  return [
    "--test-force-exit",
    ...(project.grep ?? []).map((pattern) => `--test-name-pattern=${pattern}`),
    ...(project.grepInvert ?? []).map((pattern) => `--test-skip-pattern=${pattern}`),
  ];
}
