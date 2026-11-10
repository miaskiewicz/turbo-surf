#!/usr/bin/env node
"use strict";
// Thin launcher for the native turbo-surf MCP server. Resolves the prebuilt binary
// for this platform and spawns it, forwarding argv and inheriting stdio (the server
// speaks newline-delimited JSON-RPC 2.0 over stdin/stdout). ALL engine logic —
// fetch/parse/view/crawl, the V8 JS-render tier, the MCP protocol — lives in the
// standalone Rust binary (it embeds its own V8; no Node runtime hosts it). This file
// only (a) locates the right platform binary and (b) hands off.
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

// Platform package scope — per-platform binaries install as optionalDependencies.
const PLATFORM_PACKAGES = {
  "darwin-arm64": "@miaskiewicz/turbo-surf-darwin-arm64",
  "linux-x64": "@miaskiewicz/turbo-surf-linux-x64",
  "linux-arm64": "@miaskiewicz/turbo-surf-linux-arm64",
  "win32-x64": "@miaskiewicz/turbo-surf-win32-x64",
};

// On Linux, glibc vs musl (Alpine) need different binaries but report the same
// process.platform/arch — detect musl so we pick the right one.
function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    return !process.report.getReport().header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function binaryPath() {
  const ext = process.platform === "win32" ? ".exe" : "";
  const key = `${process.platform}-${process.arch}`;

  // Per-platform optionalDependency package (the primary resolution path).
  const pkg = PLATFORM_PACKAGES[key];
  if (pkg) {
    try {
      const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
      const p = path.join(pkgDir, `turbo-surf-mcp${ext}`);
      if (fs.existsSync(p)) return p;
    } catch {}
  }

  // Legacy fallback: bundled in bin/ (pre-0.9 tarballs or local staging).
  const base = `turbo-surf-mcp-${key}`;
  const names = isMusl() ? [`${base}-musl${ext}`, `${base}${ext}`] : [`${base}${ext}`];
  for (const name of names) {
    const p = path.join(__dirname, "bin", name);
    if (fs.existsSync(p)) return p;
  }
  // dev fallback: a cargo build in this repo
  const dev = path.join(__dirname, "rust", "target", "release", `turbo-surf-mcp${ext}`);
  if (fs.existsSync(dev)) return dev;
  return null;
}

function main() {
  const bin = binaryPath();
  if (!bin) {
    console.error(
      `turbo-surf: no prebuilt binary for ${process.platform}-${process.arch}.\n` +
        `Build from source (requires Rust): cargo build --release -p turbo-surf-mcp (in the turbo-surf rust/ workspace).`,
    );
    process.exit(1);
  }
  const res = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  if (res.error) {
    console.error("turbo-surf:", res.error.message);
    process.exit(1);
  }
  process.exit(res.status == null ? 1 : res.status);
}

main();
