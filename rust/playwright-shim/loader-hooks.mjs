// Module-resolution redirect: rewrite bare `@playwright/test` / `playwright` /
// `playwright-core` imports to the napi-backed turbo-surf shim, so existing
// Playwright specs run on the no-browser Rust engine with no spec edits.
// Installed (opt-in) by `./register.mjs`.

import { existsSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, extname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SHIM = new URL("./index.mjs", import.meta.url).href;
const require = createRequire(import.meta.url);
let native;

const REDIRECTS = new Map([
  ["@playwright/test", SHIM],
  ["playwright", SHIM],
  ["playwright-core", SHIM],
]);

export function redirectTarget(specifier) {
  return REDIRECTS.get(specifier) ?? null;
}

export function resolveHook(specifier, context, nextResolve) {
  if (process.env.TURBO_PW_ATTEMPT_WORKER === "1" && specifier === "node:test")
    return { url: new URL("./attempt-test-stub.mjs", import.meta.url).href, shortCircuit: true };
  const url = redirectTarget(specifier);
  if (url) return { url, shortCircuit: true };
  const typed = typescriptTarget(specifier, context.parentURL);
  if (typed) return { url: typed, shortCircuit: true };
  return nextResolve(specifier, context);
}

function typescriptTarget(specifier, parentURL) {
  if (!parentURL || !specifier.startsWith(".")) return null;
  const target = new URL(specifier, parentURL);
  if (target.protocol !== "file:") return null;
  const path = fileURLToPath(target);
  if (existsSync(path) && statSync(path).isFile()) return null;
  const extension = extname(path);
  const candidates =
    extension === ".js"
      ? [path.slice(0, -3) + ".ts", path.slice(0, -3) + ".tsx"]
      : extension
        ? []
        : [".ts", ".tsx", ".mts", ".cts", "/index.ts"].map((suffix) => path + suffix);
  const found = candidates.find(
    (candidate) => existsSync(candidate) && statSync(candidate).isFile(),
  );
  return found ? pathToFileURL(found).href : null;
}

export function loadHook(url, context, nextLoad) {
  const extension = extname(new URL(url).pathname);
  if (!url.startsWith("file:") || ![".ts", ".tsx", ".mts", ".cts"].includes(extension)) {
    return nextLoad(url, context);
  }
  native ??= require("../crates/turbo-surf-napi/index.js");
  const filename = fileURLToPath(url);
  const transformed = native.transform(readFileSync(filename, "utf8"), true, extension === ".tsx");
  const globals = [".ts", ".tsx"].includes(extension)
    ? `import {createRequire as __turboLoaderRequire} from 'node:module';const require=__turboLoaderRequire(import.meta.url);const __filename=${JSON.stringify(filename)};const __dirname=${JSON.stringify(dirname(filename))};\n`
    : "";
  return {
    format: extension === ".cts" ? "commonjs" : "module",
    source: globals + transformed,
    shortCircuit: true,
  };
}
