// Test-only host observer: it neither changes browser arguments nor kills processes.
import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
if (typeof config.chromePath !== "string") throw new Error("Observer requires an explicit Chrome");
const ledger = join(process.argv[3], "owned-process.json");
const original = childProcess.spawn;
let captured = false;
childProcess.spawn = function (command, args, options) {
  const profile = args?.find((argument) => argument.startsWith("--user-data-dir="));
  const matches = command === config.chromePath && profile;
  if (matches && (captured || existsSync(ledger)))
    throw new Error("Test observer encountered a second owned Chrome launch");
  const child = Reflect.apply(original, this, [command, args, options]);
  if (matches) {
    captured = true;
    try {
      writeFileSync(
        ledger,
        JSON.stringify({
          pid: child.pid ?? null,
          profile: profile.slice("--user-data-dir=".length),
          executable: command,
          detached: options?.detached === true,
        }),
        { flag: "wx", mode: 0o600 },
      );
    } catch {
      // Return the child so the collector still owns cleanup if evidence storage fails.
      process.stderr.write("Owned-spawn observer could not save its private proof\n");
    }
  }
  return child;
};
syncBuiltinESMExports();
