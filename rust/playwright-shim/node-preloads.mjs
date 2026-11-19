import { realpathSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";

function optionTokens(input) {
  const tokens = [];
  let token = "";
  let quoted = false;
  let started = false;
  for (let index = 0; index < input.length; index++) {
    const character = input[index];
    if (character === '"') {
      quoted = !quoted;
      started = true;
    } else if (character === "\\" && quoted) {
      if (++index === input.length) throw new Error("Malformed NODE_OPTIONS quote");
      token += input[index];
    } else if (/\s/.test(character) && !quoted) {
      if (started) tokens.push(token);
      token = "";
      started = false;
    } else {
      token += character;
      started = true;
    }
  }
  if (quoted) throw new Error("Malformed NODE_OPTIONS quote");
  if (started) tokens.push(token);
  return tokens;
}

const valuedOptions = new Set([
  "--max-old-space-size",
  "--max-semi-space-size",
  "--stack-size",
  "--heapsnapshot-near-heap-limit",
  "--diagnostic-dir",
  "--redirect-warnings",
  "--openssl-config",
  "--icu-data-dir",
  "--env-file",
  "--env-file-if-exists",
  "--test-name-pattern",
  "--test-skip-pattern",
  "--test-reporter",
  "--test-reporter-destination",
  "--test-concurrency",
  "--test-timeout",
  "--test-shard",
  "--loader",
  "--experimental-loader",
  "--conditions",
  "-C",
  "--inspect-port",
  "--title",
  "--input-type",
  "--eval",
  "-e",
  "--print",
  "-p",
]);
function preloadPairs(tokens) {
  const pairs = [];
  for (let index = 0; index < tokens.length; index++) {
    const token = tokens[index];
    if (token === "--" || !token.startsWith("-")) break;
    const equals = token.indexOf("=");
    const flag = equals < 0 ? token : token.slice(0, equals);
    const kind = flag === "-r" ? "--require" : flag;
    if (kind === "--import" || kind === "--require") {
      const value = equals < 0 ? tokens[++index] : token.slice(equals + 1);
      if (!value || value.startsWith("--")) throw new Error(`Missing ${kind} preload`);
      pairs.push([kind, value]);
    } else if (equals < 0 && valuedOptions.has(flag)) {
      index++;
    }
  }
  return pairs;
}
function canonicalImport(value) {
  try {
    const path = value.startsWith("file:") ? fileURLToPath(value) : value;
    if (!isAbsolute(path) && !path.startsWith(".")) return null;
    try {
      return realpathSync(resolve(path));
    } catch {
      return resolve(path);
    }
  } catch {
    return null;
  }
}

export function nodePreloadArgs(
  execArgv = process.execArgv,
  nodeOptions = process.env.NODE_OPTIONS ?? "",
  excludedImports = [],
) {
  const excluded = new Set(excludedImports.map(canonicalImport).filter(Boolean));
  const seen = new Set();
  return [...preloadPairs(optionTokens(nodeOptions)), ...preloadPairs(execArgv)]
    .filter(([kind, value]) => {
      if (kind === "--import" && excluded.has(canonicalImport(value))) return false;
      const key = JSON.stringify([kind, value]);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .flat();
}
