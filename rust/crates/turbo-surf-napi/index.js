// Platform loader for the turbo-surf native (Rust) addon. Resolves the prebuilt
// `.node` for the current platform/arch (matching napi-rs binary naming), with a
// fallback to a locally-built `target/{release,debug}` cdylib for development.
//
// (napi-rs's `napi build` regenerates this file with full musl/abi detection;
// this hand-written loader covers the shipped targets + the dev build.)

const { existsSync, copyFileSync, statSync } = require("node:fs");
const { join } = require("node:path");
const { execFileSync } = require("node:child_process");

// node platform/arch → napi-rs binary suffix
const SUFFIX = {
  "darwin-arm64": "darwin-arm64",
  // darwin-x64 (Intel Mac) is no longer prebuilt — see release.yml. Unsupported
  // platforms throw a clear error; a local cargo build still backs dev use.
  "linux-x64": "linux-x64-gnu",
  "linux-arm64": "linux-arm64-gnu",
  "win32-x64": "win32-x64-msvc",
};

function suffix() {
  const key = `${process.platform}-${process.arch}`;
  const s = SUFFIX[key];
  if (!s) throw new Error(`turbo-surf-native: unsupported platform ${key}`);
  return s;
}

// dev fallback: the cdylib name cargo emits, per platform
function devCandidates() {
  const lib =
    process.platform === "win32"
      ? "turbo_surf_napi.dll"
      : process.platform === "darwin"
        ? "libturbo_surf_napi.dylib"
        : "libturbo_surf_napi.so";
  return [join(__dirname, "../../target/release", lib), join(__dirname, "../../target/debug", lib)];
}

// Node loads native addons only from a `.node` path, so copy a cargo-built
// cdylib to a sibling `.node` (refreshed when the dylib is newer) and require it.
function requireDev(dev) {
  const node = join(__dirname, "turbo-surf.dev.node");
  const stale = !existsSync(node) || statSync(dev).mtimeMs > statSync(node).mtimeMs;
  if (stale) {
    copyFileSync(dev, node);
    // macOS kills (SIGKILL) a Mach-O whose code signature doesn't match its
    // bytes; copying a freshly-built dylib invalidates it. Re-sign ad-hoc so the
    // dev addon loads. (CI ships a per-platform `.node` directly — no copy, no
    // re-sign needed.) Best-effort: skip if `codesign` is unavailable.
    if (process.platform === "darwin") {
      try {
        execFileSync("codesign", ["--force", "-s", "-", node], { stdio: "ignore" });
      } catch {
        /* codesign absent or failed — let require surface the real error */
      }
    }
  }
  return require(node);
}

function load() {
  // Explicit dev/harness selection avoids silently loading an older release dylib.
  if (process.env.TURBO_SURF_NAPI_PATH) return require(process.env.TURBO_SURF_NAPI_PATH);
  const packaged = join(__dirname, `turbo-surf.${suffix()}.node`);
  if (existsSync(packaged)) return require(packaged);
  for (const dev of devCandidates()) {
    if (existsSync(dev)) return requireDev(dev);
  }
  throw new Error(
    "turbo-surf-native: no prebuilt binary found and no local cargo build " +
      "(run `cargo build -p turbo-surf-napi` or `napi build`)",
  );
}

module.exports = load();
