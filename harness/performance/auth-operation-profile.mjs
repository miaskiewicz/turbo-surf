import { mkdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";
import { createOperationProfile } from "./operation-profile.mjs";

const output = process.env.TURBO_SURF_OPERATION_PROFILE_DIR;
const shim = process.env.TURBO_SURF_OPERATION_PROFILE_SHIM;
if (output || shim) {
  if (!output || !shim || !isAbsolute(output) || !isAbsolute(shim)) {
    throw new Error("Operation profiling requires absolute output-directory and shim paths");
  }
  const started = performance.now();
  const cpuStarted = process.cpuUsage();
  const profile = createOperationProfile();
  const { Page, Locator } = await import(pathToFileURL(shim));
  profile.wrap(
    Page.prototype,
    [
      "goto",
      "reload",
      "evaluate",
      "evaluateHandle",
      "waitForURL",
      "waitForLoadState",
      "_openLiveSession",
      "_collectSession",
      "_refreshFromSession",
      "_drainBindings",
      "_redrain",
    ],
    "Page",
  );
  profile.wrap(
    Locator.prototype,
    ["click", "fill", "press", "waitFor", "count", "getAttribute", "boundingBox", "isVisible"],
    "Locator",
  );
  mkdirSync(output, { recursive: true, mode: 0o700 });
  process.once("exit", () => {
    writeFileSync(
      join(output, `operations-${process.pid}.json`),
      JSON.stringify(
        {
          schemaVersion: 1,
          diagnosticOnly: true,
          pid: process.pid,
          elapsedMs: performance.now() - started,
          processCpuMicroseconds: process.cpuUsage(cpuStarted),
          scope:
            "Driver method elapsed time, including native work, IO and waits; not CPU attribution",
          timing:
            "Inclusive and potentially concurrent; do not sum operations or root durations as wall time",
          acceptanceEligible: false,
          operations: profile.snapshot(),
        },
        null,
        2,
      ),
      { mode: 0o600 },
    );
  });
}
