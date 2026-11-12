import { nodePreloadArgs } from "./node-preloads.mjs";
import { fork } from "node:child_process";
import { fileURLToPath } from "node:url";

const workers = new Map();
const attemptQueues = new Map();
export const isolatedAttempt = process.env.TURBO_PW_ATTEMPT_WORKER === "1";
export function validateTimeout(value) {
  if (!Number.isFinite(value) || value < 0)
    throw new Error("Test timeout must be a non-negative finite number");
  return value;
}
export function executionOptions(scopes) {
  const options = Object.assign(
    { timeout: 30000, retries: 0 },
    JSON.parse(process.env.TURBO_PW_EXECUTION ?? "{}"),
    ...scopes.map((scope) => scope.execution),
  );
  validateTimeout(options.timeout);
  if (!Number.isInteger(options.retries) || options.retries < 0)
    throw new Error("Retries must be a non-negative integer");
  return options;
}
function startWorker(entry) {
  const env = { ...process.env, TURBO_PW_ATTEMPT_WORKER: "1" };
  delete env.NODE_TEST_CONTEXT;
  delete env.NODE_OPTIONS;
  const child = fork(new URL("./attempt-worker.mjs", import.meta.url), [entry], {
    env,
    execArgv: [
      ...nodePreloadArgs(process.execArgv, process.env.NODE_OPTIONS, [
        new URL("./register.mjs", import.meta.url).href,
      ]),
      "--import",
      fileURLToPath(new URL("./register.mjs", import.meta.url)),
    ],
    stdio: ["ignore", "inherit", "inherit", "ipc"],
  });
  const worker = { child, ready: null };
  worker.ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error("Test worker startup timed out"));
    }, 30000);
    child.on("message", (msg) => {
      if (msg.kind === "ready") {
        clearTimeout(timer);
        resolve();
      }
    });
    child.once("exit", () => {
      clearTimeout(timer);
      reject(new Error("Test worker exited during startup"));
    });
    child.once("error", reject);
  });
  child.once("exit", () => {
    if (workers.get(entry) === worker) workers.delete(entry);
  });
  workers.set(entry, worker);
  return worker;
}
async function stopWorker(entry) {
  const worker = workers.get(entry);
  if (!worker) return;
  workers.delete(entry);
  const { child } = worker;
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise((resolve, reject) => {
    let cleanupError;
    const message = (msg) => {
      if (msg.kind === "closed" && msg.error) cleanupError = restoreError(msg.error);
    };
    child.on("message", message);
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      cleanupError = new Error("Worker cleanup timed out");
    }, 30000);
    child.once("exit", () => {
      clearTimeout(timer);
      child.off("message", message);
      if (cleanupError) reject(cleanupError);
      else resolve();
    });
    if (!worker.timedOut && child.connected) child.send({ kind: "close" });
  });
}
export async function closeAttemptProcesses() {
  const errors = [];
  for (const entry of workers.keys())
    try {
      await stopWorker(entry);
    } catch (error) {
      errors.push(error);
    }
  if (errors.length) throw new AggregateError(errors, "Attempt worker cleanup failed");
}

function runAttempt(worker, id, retry, timeout) {
  return new Promise((resolve, reject) => {
    let started;
    let timer;
    const expire = (message = `Timeout ${timeout}ms exceeded while running test`) => {
      const error = new Error(message);
      error.name = "TimeoutError";
      worker.timedOut = true;
      cleanup();
      worker.child.kill("SIGKILL");
      reject(error);
    };
    const reset = (value) => {
      clearTimeout(timer);
      timeout = validateTimeout(value);
      if (timeout) timer = setTimeout(expire, Math.max(1, timeout - (Date.now() - started)));
    };
    const message = (msg) => {
      if (msg.kind === "started") {
        started = msg.startedAt;
        reset(timeout);
      }
      if (msg.kind === "timeout") reset(msg.timeout);
      if (msg.kind === "result") {
        if (timeout && msg.finishedAt - started > timeout) {
          expire();
          return;
        }
        cleanup();
        if (msg.error) reject(restoreError(msg.error));
        else resolve(msg);
      }
    };
    const exit = () => {
      cleanup();
      reject(new Error("Test attempt worker exited unexpectedly"));
    };
    function cleanup() {
      clearTimeout(timer);
      worker.child.off("message", message);
      worker.child.off("exit", exit);
    }
    worker.child.on("message", message);
    worker.child.once("exit", exit);
    // Bound an unresponsive dispatcher separately from the actual test budget.
    timer = setTimeout(() => expire("Test attempt worker dispatch timed out"), 30000);
    worker.child.send({ kind: "run", id, retry, timeout });
  });
}

async function executeQueued(plan, t, options) {
  const entry = process.argv[1];
  if (!entry) throw new Error("Isolated Playwright tests require a test file entry point");
  for (let retry = 0; retry <= options.retries; retry++) {
    let worker = workers.get(entry) ?? startWorker(entry);
    try {
      await worker.ready;
      const result = await runAttempt(worker, plan.id, retry, options.timeout);
      if (result.skipped) t.skip(result.reason);
      if (retry) console.log(`[flaky] ${plan.name} passed on retry ${retry}`);
      return;
    } catch (error) {
      let failure = error;
      try {
        await stopWorker(entry);
      } catch (cleanup) {
        failure = new AggregateError([error, cleanup], "Attempt and worker cleanup failed");
      }
      if (retry === options.retries) throw failure;
    }
  }
}

function restoreError(value) {
  const error = value.errors
    ? new AggregateError(value.errors.map(restoreError), value.message)
    : new Error(value.message);
  error.stack = value.stack;
  return error;
}

export function executeIsolated(plan, t, options) {
  const entry = process.argv[1];
  const previous = attemptQueues.get(entry) ?? Promise.resolve();
  const current = previous.catch(() => {}).then(() => executeQueued(plan, t, options));
  attemptQueues.set(entry, current);
  return current;
}

process.once("exit", () => {
  for (const { child } of workers.values()) child.kill("SIGKILL");
});
