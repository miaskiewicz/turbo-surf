import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import coverage from "./js-coverage.cjs";

const root = fs.realpathSync(fileURLToPath(new URL("../", import.meta.url)));
const directory = path.resolve(root, process.argv[2] || "coverage/js");

function run(command, args, env) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd: root, env, stdio: "inherit" });
    child.once("error", (error) => {
      console.error(error);
      resolve(1);
    });
    child.once("exit", (code) => resolve(code ?? 1));
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
  const record = { root, sources: coverage.snapshot(root), commands: [] };
  const env = { ...process.env, NODE_V8_COVERAGE: raw };
  for (const [name, args] of [
    ["playwright", ["scripts/test-playwright.mjs"]],
    ["detection", ["run", "test:detection"]],
  ]) {
    const command = name === "playwright" ? process.execPath : "npm";
    const commandArgs = args;
    const exit = await run(command, commandArgs, env);
    record.commands.push({ name, command, args: commandArgs, exit });
    fs.writeFileSync(path.join(directory, "run.json"), `${JSON.stringify(record, null, 2)}\n`);
  }
  const aggregateEnv = { ...process.env };
  delete aggregateEnv.NODE_V8_COVERAGE;
  process.exitCode = await run(
    process.execPath,
    ["--expose-internals", "scripts/aggregate-js-coverage.cjs", directory],
    aggregateEnv,
  );
}

main().catch((error) => {
  console.error(error.stack);
  process.exitCode = 1;
});
