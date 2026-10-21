(async () => {
  await globalThis.__initialTimersDone;
  const semantics = {
    initialCheckpoint: Array.from(globalThis.__initialTimerEvents),
    initialRealmOrder: Array.from(globalThis.__initialRealmEvents),
  };
  const freshTask = () =>
    new Promise((resolve) => {
      const channel = new MessageChannel();
      channel.port1.onmessage = () => {
        channel.port1.close();
        channel.port2.close();
        resolve();
      };
      channel.port2.postMessage(null);
    });
  const cancellations = [];
  for (const [clear, offset] of [
    [clearTimeout, 0.75],
    [clearInterval, 4294967296.75],
  ]) {
    let fired = false;
    const id = setTimeout(() => {
      fired = true;
    }, 0);
    clear(id + offset);
    await new Promise((resolve) => setTimeout(resolve, 20));
    cancellations.push(fired);
  }
  semantics.cancellations = cancellations;
  await freshTask();
  semantics.intervalCancellation = await new Promise((resolve) => {
    let calls = 0;
    const id = setInterval(() => {
      if (++calls !== 1) return;
      clearTimeout(id + 0.75);
      setTimeout(() => {
        clearInterval(id);
        resolve(calls);
      }, 20);
    }, 0);
  });
  semantics.mathReplacement = await new Promise((resolve) => {
    const original = Math.max;
    try {
      Math.max = () => {
        throw new Error("Guest Math.max was called");
      };
      setTimeout(() => resolve({ created: true, fired: true }), 0);
    } catch (error) {
      resolve({ created: false, fired: false, errorName: error.name });
    } finally {
      Math.max = original;
    }
  });
  const cycles = 12;
  const timing = {};
  const frame = document.querySelector("#timer-frame").contentWindow;
  for (const kind of [
    "direct",
    "promise",
    "queueMicrotask",
    "crossRealm",
    "crossRealmPromise",
    "crossRealmQueueMicrotask",
  ]) {
    await freshTask();
    timing[kind] = await new Promise((resolve) => {
      const rows = [];
      const schedule = () => {
        const target = kind.startsWith("crossRealm") && rows.length % 2 ? frame : window;
        const created = performance.now();
        target.setTimeout(() => {
          rows.push(performance.now() - created);
          if (rows.length === cycles) resolve(rows);
          else if (kind === "promise" || kind === "crossRealmPromise")
            Promise.resolve().then(schedule);
          else if (kind === "queueMicrotask" || kind === "crossRealmQueueMicrotask")
            queueMicrotask(schedule);
          else schedule();
        }, 0);
      };
      schedule();
    });
  }
  await freshTask();
  timing.interval = await new Promise((resolve) => {
    const rows = [];
    let previous = performance.now();
    const id = setInterval(() => {
      const now = performance.now();
      rows.push(now - previous);
      previous = now;
      if (rows.length === cycles) {
        clearInterval(id);
        resolve(rows);
      }
    }, 0);
  });
  return JSON.stringify({
    semantics,
    protocol: {
      requestedDelayMs: 0,
      cycles,
      initialParentDelayMs: 100,
      resetSource: "MessageChannel",
    },
    timing,
    pageState: {
      visibility: document.visibilityState,
      hidden: document.hidden,
      focused: document.hasFocus(),
    },
  });
})();
