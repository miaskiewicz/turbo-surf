// Cancellation and process ownership stay in the host; none of this runs in pages.
export function collectorLifecycle({
  onAbort = () => {},
  signals = process,
  kill = process.kill,
  platform = process.platform,
  graceMs = 1500,
} = {}) {
  const controller = new AbortController();
  let browser = null,
    stopping = null;
  const stop = (signal) => {
    if (!browser?.pid) return;
    try {
      if (platform === "win32") browser.kill(signal);
      else kill(-browser.pid, signal);
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  };
  const stopBrowser = () => {
    if (stopping) return stopping;
    stopping = (async () => {
      stop("SIGTERM");
      // A leader may exit before its helpers; always retire the owned group.
      await new Promise((done) => setTimeout(done, graceMs));
      stop("SIGKILL");
    })();
    return stopping;
  };
  const abort = (signal) => {
    if (controller.signal.aborted) return;
    const error = new Error(`Chrome collector interrupted by ${signal}`);
    error.name = "AbortError";
    controller.abort(error);
    try {
      onAbort(signal);
    } catch {
      /* Cleanup must still run if checkpoint storage fails. */
    } finally {
      void stopBrowser().catch(() => {});
    }
  };
  const handlers = new Map(["SIGTERM", "SIGINT"].map((signal) => [signal, () => abort(signal)]));
  for (const [signal, handler] of handlers) signals.on(signal, handler);
  return {
    signal: controller.signal,
    own(child) {
      if (browser && browser !== child)
        throw new Error("Chrome process ownership already assigned");
      browser = child;
      if (controller.signal.aborted) {
        stop("SIGKILL");
        throw controller.signal.reason;
      }
    },
    throwIfAborted() {
      controller.signal.throwIfAborted();
    },
    race(promise) {
      if (controller.signal.aborted) return Promise.reject(controller.signal.reason);
      return new Promise((resolve, reject) => {
        const interrupted = () => {
          controller.signal.removeEventListener("abort", interrupted);
          reject(controller.signal.reason);
        };
        controller.signal.addEventListener("abort", interrupted, { once: true });
        Promise.resolve(promise)
          .then(resolve, reject)
          .finally(() => controller.signal.removeEventListener("abort", interrupted));
      });
    },
    pause(ms) {
      return new Promise((resolve, reject) => {
        if (controller.signal.aborted) return reject(controller.signal.reason);
        const interrupted = () => {
          clearTimeout(timer);
          reject(controller.signal.reason);
        };
        const timer = setTimeout(() => {
          controller.signal.removeEventListener("abort", interrupted);
          resolve();
        }, ms);
        controller.signal.addEventListener("abort", interrupted, { once: true });
      });
    },
    stopBrowser,
    dispose() {
      for (const [signal, handler] of handlers) signals.off(signal, handler);
    },
  };
}

export async function captureOwnedLaunch(childProcess, profile, launch, own) {
  const original = childProcess.spawn;
  let captured = false,
    context;
  childProcess.spawn = function (command, args, options) {
    const matches = args?.includes(`--user-data-dir=${profile}`);
    if (matches && captured) throw new Error("Multiple Chrome processes matched owned profile");
    const child = Reflect.apply(original, this, [command, args, options]);
    if (matches) {
      captured = true;
      own(child);
    }
    return child;
  };
  try {
    context = await launch();
  } finally {
    childProcess.spawn = original;
  }
  if (!captured) {
    // A changed dependency must not silently turn cleanup into an ownership guess.
    await Promise.race([
      context.close().catch(() => {}),
      new Promise((done) => setTimeout(done, 500)),
    ]);
    throw new Error(
      "Chrome ownership capture unavailable; requested context close, cleanup unverified",
    );
  }
  return context;
}
