(async () => {
  const result = {};
  for (const listenerFirst of [true, false]) {
    const rows = [],
      controller = new AbortController(),
      signal = controller.signal;
    const dependent = AbortSignal.any([signal]),
      reason = { authored: true };
    const state = () => [
      signal.aborted,
      dependent.aborted,
      signal.reason === reason,
      dependent.reason === reason,
    ];
    const listen = () =>
      signal.addEventListener("abort", () => {
        rows.push(["signal", ...state()]);
        queueMicrotask(() => rows.push(["signal-microtask", ...state()]));
      });
    dependent.addEventListener("abort", () => rows.push(["dependent", ...state()]));
    if (listenerFirst) listen();
    const readable = new ReadableStream({
      cancel(value) {
        rows.push(["cancel", value === reason, ...state()]);
      },
    });
    const writable = new WritableStream({
      abort(value) {
        rows.push(["sink-abort", value === reason, ...state()]);
      },
    });
    const pending = readable
      .pipeTo(writable, { signal })
      .catch((value) => rows.push(["pipe-reject", value === reason, ...state()]));
    if (!listenerFirst) listen();
    controller.abort(reason);
    rows.push(["after-abort", ...state()]);
    await pending;
    await Promise.resolve();
    result[listenerFirst ? "listenerBeforePipe" : "listenerAfterPipe"] = rows;
  }
  const timeout = AbortSignal.timeout(1),
    rows = [];
  timeout.addEventListener("abort", () =>
    rows.push(["timeout", timeout.aborted, timeout.reason.name]),
  );
  await new Promise((resolve) => timeout.addEventListener("abort", resolve, { once: true }));
  rows.push(["after-timeout", timeout.aborted, timeout.reason.name]);
  result.timeout = rows;
  return result;
})();
