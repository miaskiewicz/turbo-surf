(async () => {
  const object = navigator.permissions;
  const query = object.query;
  const rows = [];
  const call = async (label, receiver, args) => {
    let phase = "call",
      result;
    try {
      const promise = Reflect.apply(query, receiver, args);
      phase = "await";
      const status = await promise;
      result = { name: status.name, state: status.state };
    } catch (error) {
      result = { error: error.name, phase };
    }
    rows.push({ label, result });
  };
  await call("missing", object, []);
  for (const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["number", 1],
    ["string", "geolocation"],
    ["empty", {}],
    ["invalid", { name: "imaginary" }],
    ["symbol", { name: Symbol() }],
    ["geolocation", { name: "geolocation" }],
    ["pushMissing", { name: "push" }],
    ["pushFalse", { name: "push", userVisibleOnly: false }],
    ["pushTrue", { name: "push", userVisibleOnly: true }],
    ["midiSysex", { name: "midi", sysex: true }],
    ["storage", { name: "persistent-storage" }],
    ["background", { name: "background-sync" }],
    ["display", { name: "display-capture" }],
  ])
    await call(label, object, [value]);
  for (const name of [
    "accessibility-events",
    "accelerometer",
    "ambient-light-sensor",
    "background-fetch",
    "bluetooth",
    "captured-surface-control",
    "display-capture",
    "gyroscope",
    "idle-detection",
    "local-fonts",
    "magnetometer",
    "nfc",
    "payment-handler",
    "periodic-background-sync",
    "screen-wake-lock",
    "speaker-selection",
    "storage-access",
    "top-level-storage-access",
    "window-management",
    "window-placement",
    "xr-spatial-tracking",
    "clipboard-write",
    "clipboard-read",
    "web-printing",
    "hand-tracking",
    "keyboard-lock",
    "pointer-lock",
    "fullscreen",
    "sensors",
    "protected-media-identifier",
    "local-network-access",
    "loopback-network",
  ])
    await call("name:" + name, object, [{ name }]);
  for (const [label, receiver] of [
    ["plain", {}],
    ["nullThis", null],
    ["proxy", new Proxy(object, {})],
  ])
    await call(label, receiver, [{ name: "geolocation" }]);
  const conversions = [];
  for (const name of [
    "geolocation",
    "midi",
    "camera",
    "push",
    "clipboard-read",
    "clipboard-write",
  ]) {
    const reads = [];
    const descriptor = new Proxy(
      { name, userVisibleOnly: true },
      {
        get(target, key) {
          reads.push(String(key));
          return Reflect.get(target, key);
        },
      },
    );
    await call("conversion:" + name, object, [descriptor]);
    conversions.push({ name, reads });
  }
  const failure = new Error("original");
  let getterError = null;
  try {
    await query.call(object, {
      get name() {
        throw failure;
      },
    });
  } catch (error) {
    getterError = error === failure;
  }
  const frame = document.createElement("iframe");
  await new Promise((resolve) => {
    frame.onload = resolve;
    document.body.append(frame);
  });
  const child = frame.contentWindow;
  const realms = [];
  for (const [label, method, receiver] of [
    ["parentToChild", query, child.navigator.permissions],
    ["childToParent", child.navigator.permissions.query, object],
  ]) {
    const promise = Reflect.apply(method, receiver, [{ name: "geolocation" }]);
    const status = await promise;
    realms.push({
      label,
      parentPromise: Object.getPrototypeOf(promise) === Promise.prototype,
      parentStatus: Object.getPrototypeOf(status) === PermissionStatus.prototype,
      childStatus: Object.getPrototypeOf(status) === child.PermissionStatus.prototype,
      name: status.name,
      state: status.state,
    });
  }
  frame.remove();
  let events = 0,
    eventError = null;
  try {
    object.addEventListener("test", () => events++);
    object.dispatchEvent(new Event("test"));
  } catch (error) {
    eventError = error.name;
  }
  const prototype = Object.getPrototypeOf(object);
  return {
    rows,
    conversions,
    realms,
    getterError,
    same: navigator.permissions === object,
    parent: Object.getPrototypeOf(prototype) === EventTarget.prototype,
    constructorParent: Object.getPrototypeOf(Permissions) === EventTarget,
    own: Reflect.ownKeys(object).map(String),
    events,
    eventError,
  };
})();
