(async () => {
  // turbo-surf#103: unhandledrejection / rejectionhandled on Window, their timing against
  // a same-turn timer task, the PromiseRejectionEvent shape, preventDefault, the event
  // handler attribute, and per-realm (iframe) dispatch.
  const log = [];
  const tick = () => new Promise((resolve) => setTimeout(resolve, 20));
  const shape = (e, w) => [
    e.type,
    Object.prototype.toString.call(e),
    e instanceof w.PromiseRejectionEvent,
    e.bubbles,
    e.cancelable,
    e.isTrusted,
    e.target === w,
    e.eventPhase,
    e.reason instanceof w.Error ? e.reason.message : String(e.reason),
    Object.prototype.toString.call(e.promise),
  ];
  const listen = (w, tag) => {
    const record = (e) => log.push([tag, ...shape(e, w)]);
    w.addEventListener("unhandledrejection", record);
    w.addEventListener("rejectionhandled", record);
    return () => {
      w.removeEventListener("unhandledrejection", record);
      w.removeEventListener("rejectionhandled", record);
    };
  };
  let unlistenTop = listen(window, "top");
  // Unhandled: one event, after the current task and its microtasks.
  const late = Promise.reject(new Error("late"));
  log.push(["sync-end"]);
  setTimeout(() => log.push(["timer-0"]), 0);
  Promise.resolve().then(() => log.push(["microtask"]));
  // Handled synchronously, or within the same microtask checkpoint: no event.
  Promise.reject(new Error("sync-caught")).catch(() => {});
  const soon = Promise.reject(new Error("microtask-caught"));
  Promise.resolve().then(() => soon.catch(() => {}));
  await tick();
  // A handler attached after the report: rejectionhandled, with the same promise.
  let samePromise = null;
  window.addEventListener("rejectionhandled", (e) => (samePromise = e.promise === late), {
    once: true,
  });
  late.catch(() => {});
  log.push(["caught-late"]);
  await tick();
  log.push(["rejectionhandled-same-promise", samePromise]);
  // preventDefault: defaultPrevented and no rejectionhandled later.
  window.addEventListener("unhandledrejection", (e) => {
    if (e.reason && e.reason.message === "prevented") {
      e.preventDefault();
      log.push(["prevented", e.defaultPrevented]);
    }
  });
  const prevented = Promise.reject(new Error("prevented"));
  await tick();
  // A prevented rejection handled later (HTML: it stays in the outstanding set).
  prevented.catch(() => {});
  log.push(["caught-prevented"]);
  await tick();
  // Caught, then a derived chain left unhandled: one event, for the derived promise.
  const caught = Promise.reject(new Error("caught-then-uncaught"));
  caught.catch(() => {});
  const derived = caught.then(() => {});
  let derivedEvent = null;
  window.addEventListener("unhandledrejection", (e) => (derivedEvent = e.promise === derived), {
    once: true,
  });
  await tick();
  log.push(["caught-then-uncaught-derived", derivedEvent]);
  // Page getters on Object.prototype must not reach the host's event init.
  const touched = [];
  for (const key of ["bubbles", "cancelable", "composed", "promise", "reason"])
    Object.defineProperty(Object.prototype, key, {
      configurable: true,
      get() {
        touched.push(key);
        return true;
      },
    });
  Promise.reject(new Error("proto-getters"));
  await tick();
  for (const key of ["bubbles", "cancelable", "composed", "promise", "reason"])
    delete Object.prototype[key];
  log.push(["proto-getters-touched", touched]);
  // Many rejections in one task: one event each, in order.
  const stress = [];
  const onStress = (e) => {
    if (e.reason && e.reason.stress !== undefined) stress.push(e.reason.stress);
  };
  unlistenTop();
  window.addEventListener("unhandledrejection", onStress);
  for (let i = 0; i < 200; i++) Promise.reject({ stress: i });
  await tick();
  window.removeEventListener("unhandledrejection", onStress);
  unlistenTop = listen(window, "top");
  log.push(["stress", stress.length, stress.every((value, index) => value === index)]);
  // The onunhandledrejection attribute, and a non-Error reason.
  window.onunhandledrejection = (e) => log.push(["attribute", String(e.reason)]);
  Promise.reject(42);
  await tick();
  window.onunhandledrejection = null;
  // Constructor.
  const made = new PromiseRejectionEvent("unhandledrejection", {
    promise: late,
    reason: 7,
    cancelable: true,
  });
  log.push(["constructed", made.promise === late, made.reason, made.cancelable, made.isTrusted]);
  let missing;
  try {
    new PromiseRejectionEvent("x", {});
    missing = "no-throw";
  } catch (error) {
    missing = error.name;
  }
  log.push(["constructor-without-promise", missing]);
  // Per realm: a rejection created in an iframe realm fires on the iframe's window.
  const frame = document.createElement("iframe");
  document.body.append(frame);
  listen(frame.contentWindow, "frame");
  frame.contentWindow.Promise.reject(new frame.contentWindow.Error("frame"));
  // Waits for the frame realm's own task, however long creating the frame took.
  for (let turn = 0; turn < 50 && !log.some((entry) => entry[0] === "frame"); turn++) await tick();
  // A rejection in a removed frame's realm: no event on either window.
  const removed = frame.contentWindow;
  frame.remove();
  const before = log.length;
  removed.Promise.reject(new removed.Error("after-remove"));
  await tick();
  await tick();
  log.push(["after-remove-events", log.length - before]);
  return log;
})();
