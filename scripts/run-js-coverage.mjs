import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import coverage from "./js-coverage.cjs";
import completeness from "./js-test-completeness.cjs";

const root = fs.realpathSync(fileURLToPath(new URL("../", import.meta.url)));
const directory = path.resolve(root, process.argv[2] || "coverage/js");

function run(command, args, env, name) {
  const log = path.join(directory, `${name}.log`);
  const descriptor = fs.openSync(log, "wx");
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd: root, env, stdio: ["ignore", "pipe", "pipe"] });
    let failed = false;
    const output = (stream) => (chunk) => {
      fs.writeSync(descriptor, chunk);
      stream.write(chunk);
    };
    child.stdout.on("data", output(process.stdout));
    child.stderr.on("data", output(process.stderr));
    child.once("error", (error) => {
      failed = true;
      fs.writeSync(descriptor, error.stack);
    });
    child.once("close", (code) => {
      fs.closeSync(descriptor);
      resolve({ exit: failed ? 1 : (code ?? 1), log, logSha256: coverage.sha(log) });
    });
  });
}

async function main() {
  if (process.version !== coverage.scope.nodeVersion) {
    throw new Error(`Coverage requires Node ${coverage.scope.nodeVersion}`);
  }
  // A new directory prevents mixing stale reports or a different source epoch.
  fs.mkdirSync(path.dirname(directory), { recursive: true });
  fs.mkdirSync(directory);
  const raw = path.join(directory, "raw");
  fs.mkdirSync(raw);
  const record = {
    root,
    sources: coverage.snapshot(root),
    ...completeness.testInventory(root),
    commands: [],
  };
  const env = {
    ...process.env,
    NODE_V8_COVERAGE: raw,
    TURBO_SURF_JS_COVERAGE_TAP: "1",
  };
  record.executionInputs = completeness.executionInputs(root, env);
  for (const [name, args] of [
    ["playwright", ["scripts/test-playwright.mjs"]],
    ["detection", ["run", "test:detection"]],
  ]) {
    const command = name === "playwright" ? process.execPath : "npm";
    const commandArgs = args;
    const result = await run(command, commandArgs, env, name);
    record.commands.push({ name, command, args: commandArgs, ...result });
    fs.writeFileSync(path.join(directory, "run.json"), `${JSON.stringify(record, null, 2)}\n`);
  }
  const aggregateEnv = { ...process.env };
  delete aggregateEnv.NODE_V8_COVERAGE;
  const result = await run(
    process.execPath,
    ["--expose-internals", "scripts/aggregate-js-coverage.cjs", directory],
    aggregateEnv,
    "aggregate",
  );
  process.exitCode = result.exit;
}

main().catch((error) => {
  console.error(error.stack);
  process.exitCode = 1;
});
