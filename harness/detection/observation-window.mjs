export function observationMilliseconds(config) {
  const value = Object.hasOwn(config, "observationMs")
    ? config.observationMs
    : Object.hasOwn(config, "waitMs")
      ? config.waitMs
      : 2500;
  if (!Number.isSafeInteger(value) || value < 0 || value > 60000)
    throw new Error("observationMs must be an integer from 0 to 60000 milliseconds");
  return value;
}

export function observationTriggerIndex(steps) {
  return steps.findLastIndex((step) => step.press === "Enter");
}

export function startObservation(durationMs, triggerStepIndex, clock) {
  if (!Number.isSafeInteger(durationMs) || durationMs < 0 || durationMs > 60000)
    throw new Error("Observation duration must be an integer from 0 to 60000 milliseconds");
  const started = clock.monotonic();
  const startedMs = clock.wall();
  return {
    deadline: started + durationMs,
    started,
    record: {
      policy: "fixed-deadline-before-enter",
      source: "host-monotonic-clock",
      triggerStepIndex,
      requestedMs: durationMs,
      startedMs,
      deadlineMs: startedMs + durationMs,
      completed: false,
    },
  };
}

export function remainingObservationMs(window, clock) {
  return Math.max(0, window.deadline - clock.monotonic());
}

// Neither HTTP status nor a result marker changes the deadline. A successful
// shell, delayed redirect and error response receive the same observation time.
export async function waitForObservation(window, clock, pause, pump = async () => {}) {
  let remaining;
  while ((remaining = remainingObservationMs(window, clock)) > 0) {
    await pause(Math.min(remaining, 25));
    await pump();
  }
}

// Call after the final document snapshot. Blocking input/navigation or a stalled
// snapshot can overrun the fixed deadline; preserve that fact rather than renew it.
export function finishObservation(window, clock, completed, extra = {}) {
  const elapsedMs = Math.max(0, clock.monotonic() - window.started);
  Object.assign(window.record, extra, {
    finishedMs: clock.wall(),
    elapsedMs,
    overrunMs: Math.max(0, elapsedMs - window.record.requestedMs),
    completed,
  });
  return window.record;
}

export async function captureStableDocument(identity, capture) {
  for (let attempt = 0; attempt < 3; attempt++) {
    const before = await identity();
    try {
      const value = await capture();
      if (before === (await identity())) return { value, retries: attempt };
    } catch (error) {
      if (
        !/Execution context was destroyed|Cannot find context with specified id|Cannot find default execution context|Unable to retrieve content because the page is navigating/.test(
          String(error),
        )
      )
        throw error;
    }
  }
  throw new Error("Main document changed during all 3 snapshot attempts");
}
