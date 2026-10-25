((options) => {
  // Installs the trusted pointer sequence log for one pointer-capture case. Every element of
  // the case markup and the window listen, so Chrome also dispatches enter/leave events.
  const log = [];
  globalThis.__log = log;
  globalThis.$ = (id) => document.getElementById(id);
  globalThis.sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  globalThis.settle = (label, promise) =>
    promise.then(
      (value) => log.push(`${label} resolved ${value}`),
      (error) => log.push(`${label} rejected ${error.name}: ${error.message}`),
    );
  const name = (node) =>
    node === null || node === undefined
      ? "null"
      : node === document
        ? "#document"
        : node === window
          ? "window"
          : node.id || node.nodeName;
  globalThis.nameOf = name;
  const boundary = /^(pointer|mouse)(over|out|enter|leave)$/;
  const scoped = /(enter|leave)$/;
  const moving = /^(pointer|mouse)(move|down|up)$|capture$/;
  const types = [
    "pointerover",
    "pointerenter",
    "pointerdown",
    "pointermove",
    "pointerup",
    "pointercancel",
    "pointerout",
    "pointerleave",
    "gotpointercapture",
    "lostpointercapture",
    "mouseover",
    "mouseenter",
    "mousedown",
    "mousemove",
    "mouseup",
    "mouseout",
    "mouseleave",
    "click",
  ];
  const record = (event) => {
    if (options.boundary === false && boundary.test(event.type)) return;
    const row = [event.type, name(event.target)];
    if (event.relatedTarget !== undefined && boundary.test(event.type))
      row.push(`rel=${name(event.relatedTarget)}`);
    row.push(`b=${event.buttons}`);
    if (moving.test(event.type) || event.type === "click") row.push(`btn=${event.button}`);
    if (options.movement && moving.test(event.type))
      row.push(`m=${event.movementX},${event.movementY}`, `c=${event.clientX},${event.clientY}`);
    if (options.flags)
      row.push(
        `f=${event.bubbles},${event.cancelable},${event.composed},${event.detail},${event.isTrusted}`,
        event.constructor.name,
      );
    if (event.pointerId !== undefined && options.flags)
      row.push(`p=${event.pointerId},${event.pointerType},${event.isPrimary},${event.pressure}`);
    log.push(row.join(" "));
  };
  for (const type of types) {
    if (scoped.test(type)) {
      for (const node of document.querySelectorAll("*"))
        node.addEventListener(type, (event) => {
          if (event.currentTarget === event.target) record(event);
        });
    } else window.addEventListener(type, record, true);
  }
  for (const type of ["pointerlockchange", "pointerlockerror"])
    document.addEventListener(type, (event) =>
      log.push(`${type} ${name(event.target)} ${event.isTrusted} lock=${name(document.pointerLockElement)}`),
    );
})
