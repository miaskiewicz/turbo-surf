import { spawn } from "node:child_process";

const env = { ...process.env };
const args = process.argv.slice(2);
if (env.TURBO_SURF_JS_COVERAGE_TAP === "1") {
  args.unshift("--test-reporter=tap");
}
delete env.TURBO_SURF_JS_COVERAGE_TAP;
const child = spawn(process.execPath, args, { env, stdio: "inherit" });
let failed = false;
child.once("error", (error) => {
  failed = true;
  console.error(error);
});
child.once("close", (code) => {
  process.exitCode = failed ? 1 : (code ?? 1);
});
