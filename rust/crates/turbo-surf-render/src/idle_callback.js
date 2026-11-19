// Expired requests have no idle budget, even when the main thread delayed delivery.
globalThis.requestIdleCallback = (fn, options = {}) => {
  if (typeof fn !== "function") throw new TypeError("Idle callback must be a function");
  const timeout = Number(options?.timeout);
  const expires = timeout > 0 ? __perfNow() + timeout : Infinity;
  return globalThis.setTimeout(() => {
    const start = __perfNow();
    const didTimeout = start >= expires;
    fn({
      didTimeout,
      timeRemaining: () => (didTimeout ? 0 : Math.max(0, 50 - (__perfNow() - start))),
    });
  }, 1);
};
globalThis.cancelIdleCallback = (id) => globalThis.clearTimeout(id);
