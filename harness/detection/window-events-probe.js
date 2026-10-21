// Authored localhost control: actual Node → Document → Window event behavior.
(() => {
  const result = {};
  const node = document.createElement("span");
  document.body.append(node);
  const label = (target) =>
    target === window
      ? "window"
      : target === document
        ? "document"
        : target === node
          ? "node"
          : target.nodeName;
  const rows = [];
  for (const [target, name, capture] of [
    [window, "window-capture", true],
    [document, "document-capture", true],
    [node, "target-bubble", false],
    [node, "target-capture", true],
    [document, "document-bubble", false],
    [window, "window-bubble", false],
  ]) {
    target.addEventListener(
      "probe",
      function (event) {
        rows.push([
          name,
          event.eventPhase,
          event.currentTarget === this,
          event.target === node,
          event.composedPath().map(label),
        ]);
      },
      capture,
    );
  }
  const event = new Event("probe", { bubbles: true });
  node.dispatchEvent(event);
  result.phases = rows;
  result.cleanup = [
    event.currentTarget === null,
    event.eventPhase,
    event.composedPath().length,
    event.target === node,
  ];
  result.resources = [];
  for (const type of ["error", "load"])
    for (const [target, name] of [
      [window, "window"],
      [document, "document"],
      [node, "node"],
    ])
      for (const capture of [true, false])
        target.addEventListener(
          type,
          (event) => result.resources.push([type, name, capture, event.eventPhase]),
          capture,
        );
  node.dispatchEvent(new Event("error"));
  node.dispatchEvent(new Event("load", { bubbles: true }));
  result.abort = [];
  for (const [target, name] of [
    [node, "node"],
    [window, "window"],
  ]) {
    const controller = new AbortController();
    const callback = () => result.abort.push(name);
    target.addEventListener("signal", callback, { signal: controller.signal });
    controller.abort("reason");
    target.addEventListener("signal", callback);
    target.dispatchEvent(new Event("signal"));
    target.removeEventListener("signal", callback);
  }
  const failure = new Error("boom");
  window.onerror = function (message, source, line, column, error) {
    result.onerror = [
      message,
      source,
      line,
      column,
      error === failure,
      arguments.length,
      this === window,
    ];
    return true;
  };
  const error = new ErrorEvent("error", {
    message: "boom",
    filename: "fixture.js",
    lineno: 4,
    colno: 7,
    error: failure,
    cancelable: true,
  });
  result.onerrorCancelled = !window.dispatchEvent(error) && error.defaultPrevented;
  window.onerror = function (event) {
    result.resourceOnerror = [arguments.length, event instanceof Event];
    return false;
  };
  const resource = new Event("error", { cancelable: true });
  result.resourceOnerrorCancelled = !window.dispatchEvent(resource);
  window.onerror = null;
  result.passive = [];
  node.addEventListener(
    "passive",
    (event) => {
      event.preventDefault();
      event.returnValue = false;
      result.passive.push(event.defaultPrevented);
    },
    { passive: true },
  );
  const passive = new Event("passive", { cancelable: true });
  result.passive.push(node.dispatchEvent(passive), passive.defaultPrevented);
  const inert = new DOMParser().parseFromString("<body><div></div>", "text/html");
  let inertHits = 0;
  const observer = () => inertHits++;
  window.addEventListener("inert", observer);
  inert.body.firstChild.dispatchEvent(new Event("inert", { bubbles: true }));
  window.removeEventListener("inert", observer);
  result.inertWindowHits = inertHits;
  node.remove();
  return result;
})();
