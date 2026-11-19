import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  linkSync,
  openSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

const libraries = {
  darwin: "libturbo_surf_napi.dylib",
  linux: "libturbo_surf_napi.so",
  win32: "turbo_surf_napi.dll",
};

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// Only the build-and-test gate uses this selector. Published-package resolution
// and the bare shim tests retain the loader's existing preference and skip rules.
export function localAddon(root, env = process.env, platform = process.platform) {
  if (env.TURBO_SURF_NAPI_PATH) {
    const path = resolve(env.TURBO_SURF_NAPI_PATH);
    if (!existsSync(path)) throw new Error(`Explicit native addon does not exist: ${path}`);
    const digest = sha256(readFileSync(path));
    return { path, source: path, sourceSha256: digest, artifactSha256: digest, explicit: true };
  }
  const library = libraries[platform];
  if (!library) throw new Error(`Unsupported local native addon platform: ${platform}`);
  const target = resolve(root, "rust", env.CARGO_TARGET_DIR || "target");
  const source = join(target, env.CARGO_BUILD_TARGET || "", "release", library);
  if (!existsSync(source))
    throw new Error(`Local release addon is missing: ${source}; run npm run build:addon`);
  const bytes = readFileSync(source);
  const sourceSha256 = sha256(bytes);
  const temporary = join(dirname(source), `turbo-surf.test-${randomUUID()}.tmp.node`);
  let temporaryOwned = false;
  try {
    const descriptor = openSync(temporary, "wx");
    temporaryOwned = true;
    try {
      writeFileSync(descriptor, bytes);
    } finally {
      closeSync(descriptor);
    }
    if (platform === "darwin") {
      try {
        execFileSync(
          "codesign",
          [
            "--force",
            "-s",
            "-",
            "--identifier",
            "turbo-surf.local-test-addon",
            "--timestamp=none",
            temporary,
          ],
          { stdio: "ignore" },
        );
      } catch {
        // Match the development loader: require reports actual loading failures.
      }
    }
    const artifactSha256 = sha256(readFileSync(temporary));
    const path = join(dirname(source), `turbo-surf.test-${artifactSha256}.node`);
    try {
      linkSync(temporary, path);
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (sha256(readFileSync(path)) !== artifactSha256)
        throw new Error(`Existing native addon snapshot has invalid content: ${path}`);
    }
    return { path, source, sourceSha256, artifactSha256, explicit: false };
  } finally {
    if (temporaryOwned && existsSync(temporary)) unlinkSync(temporary);
  }
}
