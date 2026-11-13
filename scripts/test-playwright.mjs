import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { localAddon } from "./local-addon.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const addon = localAddon(root);
const env = { ...process.env, TURBO_SURF_NAPI_PATH: addon.path };
console.error(`Playwright native addon: ${addon.source} -> ${addon.path}`);

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, env, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
}

const controls = await run(process.execPath, [
  "--test",
  fileURLToPath(new URL("./local-addon.test.mjs", import.meta.url)),
]);
process.exitCode = controls;
if (controls === 0) {
  for (const script of ["test:shim", "test:e2e"]) {
    // Keep the existing gate's process-group deadline and child shutdown behavior.
    const code = await run("timeout", ["--signal=KILL", "600", "npm", "run", script]);
    if (code !== 0) {
      process.exitCode = code;
      break;
    }
  }
}
