#!/usr/bin/env node
// `turbo-surf test` entry — the shim's answer to `npx playwright test`. Reads a
// `playwright.config.*`, boots any `webServer`s, and runs each project as a
// `node --import ../register.mjs --test …` sweep. See config.mjs for scope.

import { spawn } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import {
  discoverTests,
  findConfig,
  loadConfig,
  startWebServers,
  stopWebServers,
} from "../config.mjs";

const SHIM_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REGISTER = resolve(SHIM_DIR, "register.mjs");

export async function run(argv) {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      config: { type: "string", short: "c" },
      project: { type: "string", multiple: true },
      reporter: { type: "string" },
      "list-only": { type: "boolean" },
      help: { type: "boolean", short: "h" },
    },
  });
  if (values.help) {
    printHelp();
    return 0;
  }
  const configPath = values.config ?? findConfig();
  if (!configPath) {
    console.error("turbo-surf-test: no playwright.config.* found (pass --config)");
    return 2;
  }
  const config = await loadConfig(configPath);
  const reporter = values.reporter ? [[values.reporter]] : config.reporter;
  const chosen = values.project
    ? config.projects.filter((p) => values.project.includes(p.name))
    : config.projects;
  if (!chosen.length) {
    console.error(`turbo-surf-test: no projects matched ${JSON.stringify(values.project)}`);
    return 2;
  }
  const webHandles = await startWebServers(config.webServer, (m) =>
    console.log(`[webServer] ${m}`),
  );
  let failed = 0;
  try {
    for (const project of chosen) {
      const files = positionals.length
        ? positionals.map((p) => resolve(p))
        : discoverTests(project);
      if (!files.length) {
        console.log(`[${project.name}] no test files matched`);
        continue;
      }
      if (values["list-only"]) {
        for (const f of files) console.log(`[${project.name}] ${f}`);
        continue;
      }
      const code = await runProject(project, files, reporter);
      if (code !== 0) failed++;
    }
  } finally {
    await stopWebServers(webHandles);
  }
  return failed ? 1 : 0;
}

function runProject(project, files, reporter) {
  const use = project.use ?? {};
  // Scrub `node --test` / node-runner state that our parent may be propagating
  // — leaving it in confuses node:test in the grandchild ("run() is being
  // called recursively, skipping files"), so no tests run at all.
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.NODE_OPTIONS;
  env.TURBO_PW_PROJECT_NAME = project.name;
  env.TURBO_PW_USE = JSON.stringify(use);
  // Playwright's canonical `use` keys that the shim already reads — forward them
  // so fixtures see the merged project config without a code change in-suite.
  if (use.baseURL) env.TURBO_SHIM_BASE_URL = use.baseURL;
  if (use.locale) env.TURBO_SHIM_LOCALE = use.locale;
  if (use.userAgent) env.TURBO_SHIM_USER_AGENT = use.userAgent;
  if (use.testIdAttribute) env.TURBO_SHIM_TESTID_ATTR = use.testIdAttribute;

  const kind = reporter[0]?.[0] ?? "list";
  const captureTap = kind === "html";
  const nodeReporter = reporterToNode(kind);
  const args = [
    "--import",
    REGISTER,
    "--test",
    `--test-reporter=${nodeReporter}`,
    "--test-reporter-destination=stdout",
    ...files,
  ];
  return new Promise((resolveP) => {
    let tap = "";
    // Pipe stdout so the HTML path can capture TAP, and relay the stream either
    // way — spawning the grandchild as `stdio: "inherit"` drops output when the
    // CLI itself is spawned from a `node:test` harness.
    const child = spawn(process.execPath, args, {
      env,
      stdio: ["inherit", "pipe", "inherit"],
    });
    child.stdout.on("data", (b) => {
      const s = b.toString("utf8");
      if (captureTap) tap += s;
      process.stdout.write(s);
    });
    child.on("close", async (code) => {
      if (captureTap) await writeHtmlReport(tap, reporter[0]?.[1] ?? {});
      resolveP(code ?? 1);
    });
  });
}

function reporterToNode(kind) {
  if (kind === "spec" || kind === "list") return "spec";
  if (kind === "tap") return "tap";
  if (kind === "dot") return "dot";
  // `html` captures TAP first (we write HTML ourselves afterwards).
  if (kind === "html") return "tap";
  return "spec";
}

async function writeHtmlReport(tap, opts) {
  const outFile = opts.outputFile ?? "playwright-report.html";
  const rows = [];
  let pass = 0;
  let fail = 0;
  let skip = 0;
  // A plain node:test TAP line looks like `ok 1 - name` or `not ok 1 - name # SKIP`.
  const line = /^\s*(ok|not ok)\s+\d+\s*-?\s*(.*)$/m;
  for (const raw of tap.split("\n")) {
    const m = line.exec(raw);
    if (!m) continue;
    const ok = m[1] === "ok";
    const skipped = /#\s*SKIP/i.test(m[2]);
    const name = m[2].replace(/#\s*SKIP.*$/i, "").trim();
    if (skipped) skip++;
    else if (ok) pass++;
    else fail++;
    const cls = skipped ? "s" : ok ? "p" : "f";
    const label = skipped ? "skip" : ok ? "pass" : "fail";
    rows.push(`<tr class="${cls}"><td>${label}</td><td>${escapeHtml(name)}</td></tr>`);
  }
  const html = `<!doctype html><meta charset=utf-8><title>turbo-surf test report</title>
<style>body{font:14px system-ui,sans-serif;margin:20px}
.p td:first-child{color:#2a6}.f td:first-child{color:#c22}.s td:first-child{color:#888}
table{border-collapse:collapse}td{padding:4px 10px;border-bottom:1px solid #eee}</style>
<h1>turbo-surf test report</h1>
<p>${pass} passed · ${fail} failed · ${skip} skipped</p>
<table>${rows.join("")}</table>`;
  await writeFile(outFile, html, "utf8");
  console.log(`[reporter html] ${outFile}`);
}

function escapeHtml(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}

function printHelp() {
  console.log(`turbo-surf-test — run a Playwright suite through the turbo-surf shim

usage: turbo-surf-test [options] [files...]

  -c, --config <path>    playwright.config.{ts,mts,js,mjs,cjs} (auto-detected)
      --project <name>   run only projects with this name (repeatable)
      --reporter <name>  list | spec | tap | dot | html (overrides config)
      --list-only        print the files that would run, don't run them
  -h, --help             show this help
`);
}

// Direct invocation entry (not when imported as a library).
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  run(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
      console.error(e);
      process.exit(1);
    },
  );
}
