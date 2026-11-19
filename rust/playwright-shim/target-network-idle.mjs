import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";

export async function waitForTargetNetworkIdle(
  read,
  { timeout = 30000, quiet = 500, now = () => performance.now(), pause = sleep } = {},
) {
  const started = now();
  let revision;
  let quietSince;
  for (;;) {
    const state = await read();
    validateState(state);
    const time = now();
    if (state.pendingRequests || state.pendingNavigation) quietSince = undefined;
    else if (revision !== state.revision || quietSince == null) quietSince = time;
    revision = state.revision;
    if (timeout > 0 && time - started >= timeout) {
      const error = new Error(`Timeout ${timeout}ms exceeded waiting for target networkidle`);
      error.name = "TimeoutError";
      throw error;
    }
    if (quietSince != null && time - quietSince >= quiet) return;
    await pause(25);
  }
}

function validateState(state) {
  if (state == null || typeof state.closed !== "boolean")
    throw new Error("Invalid native target networkidle evidence");
  if (state.error) throw new Error(`Native target networkidle unavailable: ${state.error}`);
  if (state.closed) throw new Error("Target page has been closed");
  if (
    !Number.isSafeInteger(state.pendingRequests) ||
    state.pendingRequests < 0 ||
    typeof state.pendingNavigation !== "boolean" ||
    typeof state.revision !== "string"
  )
    throw new Error("Invalid native target networkidle evidence");
}
