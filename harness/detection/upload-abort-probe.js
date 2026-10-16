(async () => {
  let cancelled;
  const aborter = new AbortController();
  const reason = { authored: "stalled-peer" };
  const body = new ReadableStream({
    start(controller) {
      controller.enqueue(new Uint8Array([65]));
    },
    cancel(value) {
      cancelled = value;
    },
  });
  const pending = fetch("/upload-hold?id=stalled-abort", {
    method: "POST",
    body,
    duplex: "half",
    signal: aborter.signal,
  });
  await new Promise((resolve) => setTimeout(resolve, 25));
  const started = performance.now();
  const rejection = pending.then(
    () => false,
    (error) => error === reason,
  );
  aborter.abort(reason);
  const rejectedWithReason = await rejection;
  const rejectedWithoutWaitingForCleanup = performance.now() - started < 75;
  await fetch("/release?id=stalled-abort");
  return {
    rejectedWithReason,
    rejectedWithoutWaitingForCleanup,
    sourceReasonIdentity: cancelled === reason,
  };
})();
