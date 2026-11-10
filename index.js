"use strict";
// Programmatic entry: locate + spawn the native turbo-surf MCP binary. The CLI
// (`cli.js`, the `turbo-surf-mcp` bin) is the primary interface; this exists so
// `require("turbo-surf")` works in scripts. The engine is the
// standalone Rust binary — this package is only the platform-binary launcher.
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

// Platform package scope — per-platform binaries install as optionalDependencies.
const PLATFORM_PACKAGES = {
  "darwin-arm64": "@miaskiewicz/turbo-surf-darwin-arm64",
  "linux-x64": "@miaskiewicz/turbo-surf-linux-x64",
  "linux-arm64": "@miaskiewicz/turbo-surf-linux-arm64",
  "win32-x64": "@miaskiewicz/turbo-surf-win32-x64",
};

function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    return !process.report.getReport().header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

/** Absolute path to the platform's turbo-surf-mcp binary, or null if absent. */
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
  const dev = path.join(__dirname, "rust", "target", "release", `turbo-surf-mcp${ext}`);
  if (fs.existsSync(dev)) return dev;
  return null;
}

/** Spawn the MCP server (stdio JSON-RPC). Returns the ChildProcess. */
function spawnMcp(args = [], opts = {}) {
  const bin = binaryPath();
  if (!bin) throw new Error(`turbo-surf: no binary for ${process.platform}-${process.arch}`);
  return spawn(bin, args, { stdio: "inherit", ...opts });
}

module.exports = { binaryPath, spawnMcp };
