import { spawn } from "node:child_process";

export function superviseTestChild(t, command, args, options) {
  const pending = superviseChild(command, args, { ...options, signal: t.signal });
  // node:test can cancel the body before its awaits/finally finish. Its after
  // hooks still block the next case until owned process cleanup has completed.
  t.after(() => pending);
  return pending;
}

// The grace period permits cleanup after a failed deadline; it never turns that
// failure into a successful capture. Collectors own any separately detached browser.
export function superviseChild(command, args, options) {
  const { signal, timeoutMs, killGraceMs = 5000, maxOutputBytes = 1024 * 1024, onOutput } = options;
  for (const [name, value] of Object.entries({ timeoutMs, killGraceMs, maxOutputBytes }))
    if (!Number.isSafeInteger(value) || value < 1 || value > 2 ** 31 - 1)
      throw new TypeError(`${name} must be a positive 32-bit integer`);
  if (onOutput !== undefined && typeof onOutput !== "function")
    throw new TypeError("onOutput must be a function");
  return new Promise((done) => {
    const result = {
      code: null,
      signal: null,
      timedOut: false,
      aborted: Boolean(signal?.aborted),
      error: null,
      stdout: "",
      stderr: "",
    };
    if (result.aborted) return done(result);
    let child;
    try {
      child = spawn(command, args, {
        ...options.spawnOptions,
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      result.error = String(error);
      return done(result);
    }
    const output = { stdout: Buffer.alloc(0), stderr: Buffer.alloc(0) };
    let deadlineTimer;
    let retirement;
    let stopping = false;
    let outputFailed = false;
    const kill = (terminationSignal) => {
      try {
        if (process.platform === "win32") child.kill(terminationSignal);
        else if (child.pid) process.kill(-child.pid, terminationSignal);
      } catch (error) {
        if (error.code !== "ESRCH") result.error ??= String(error);
      }
    };
    const stop = (reason) => {
      if (reason) result[reason] = true;
      if (stopping) return;
      stopping = true;
      clearTimeout(deadlineTimer);
      kill("SIGTERM");
      retirement = new Promise((retired) => {
        setTimeout(() => {
          // Helpers may outlive a leader which closed all of its own stdio.
          kill("SIGKILL");
          retired();
        }, killGraceMs);
      });
    };
    const abort = () => stop("aborted");
    for (const name of Object.keys(output))
      child[name].on("data", (bytes) => {
        output[name] = Buffer.concat([output[name], bytes]).subarray(-maxOutputBytes);
        if (outputFailed) return;
        try {
          onOutput?.(name, bytes);
        } catch (error) {
          outputFailed = true;
          result.error ??= `Output callback failed: ${error}`;
          stop();
        }
      });
    child.on("error", (error) => {
      result.error ??= String(error);
    });
    child.on("close", async (code, terminationSignal) => {
      clearTimeout(deadlineTimer);
      signal?.removeEventListener("abort", abort);
      if (retirement) await retirement;
      done({
        ...result,
        code,
        signal: terminationSignal,
        stdout: output.stdout.toString(),
        stderr: output.stderr.toString(),
      });
    });
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) abort();
    else deadlineTimer = setTimeout(() => stop("timedOut"), timeoutMs);
  });
}
