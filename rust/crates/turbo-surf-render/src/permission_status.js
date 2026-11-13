const __permissionStatusRecords = __interfaceMap("PermissionStatus");
const __permissionStatusLive = new Set(),
  __permissionStatusWeakRef = WeakRef;
const __permissionStatusDereference = WeakRef.prototype.deref,
  __permissionStatusEach = Set.prototype.forEach.bind(__permissionStatusLive),
  __permissionStatusDelete = Set.prototype.delete.bind(__permissionStatusLive),
  __permissionStatusAdd = Set.prototype.add.bind(__permissionStatusLive);
const __permissionStatusSize = Object.getOwnPropertyDescriptor(Set.prototype, "size").get.bind(
  __permissionStatusLive,
);
const __permissionStatusTarget = globalThis.EventTarget,
  __permissionStatusDefine = Object.defineProperty,
  __permissionStatusSetPrototype = Object.setPrototypeOf,
  __permissionStatusApply = Reflect.apply,
  __permissionStatusTypeError = TypeError,
  __permissionStatusListen = EventTarget.prototype.addEventListener,
  __permissionStatusUnlisten = EventTarget.prototype.removeEventListener;
const __permissionStatusRecord = (receiver) => {
  const state = __permissionStatusRecords.get(receiver);
  if (!state) throw new __permissionStatusTypeError("Illegal invocation");
  return state;
};
const __permissionStatusState = (state) => {
  if (!ops.op_window_active(state.document)) return state.lastState;
  const configured = ops.op_context_emulation(state.document, state.feature);
  if (!configured.active) return state.lastState;
  state.lastState =
    ops.op_permissions_policy(state.document, state.feature, "", 4) &&
    !ops.op_permissions_policy(state.document, state.feature, "", 2)
      ? "denied"
      : configured.permission || state.baseState;
  return state.lastState;
};
class PermissionStatus extends __permissionStatusTarget {
  constructor() {
    throw new __permissionStatusTypeError("Illegal constructor");
  }
  get name() {
    return __permissionStatusRecord(this).name;
  }
  get state() {
    return __permissionStatusState(__permissionStatusRecord(this));
  }
  get onchange() {
    return __permissionStatusRecord(this).handler;
  }
  set onchange(value) {
    const state = __permissionStatusRecord(this);
    state.handler =
      value !== null && (typeof value === "object" || typeof value === "function") ? value : null;
    if (state.handler !== null && !state.listener) {
      state.listener = (event) => {
        if (typeof state.handler === "function")
          __permissionStatusApply(state.handler, this, [event]);
      };
      __permissionStatusApply(__permissionStatusListen, this, ["change", state.listener]);
    } else if (state.handler === null && state.listener) {
      __permissionStatusApply(__permissionStatusUnlisten, this, ["change", state.listener]);
      state.listener = null;
    }
  }
}
delete PermissionStatus.prototype.constructor;
for (const name of ["name", "state", "onchange"]) {
  const descriptor = Object.getOwnPropertyDescriptor(PermissionStatus.prototype, name);
  descriptor.enumerable = true;
  descriptor.get = __documentFunction(descriptor.get, "get " + name, 0);
  if (descriptor.set) descriptor.set = __documentFunction(descriptor.set, "set " + name, 1);
  __permissionStatusDefine(PermissionStatus.prototype, name, descriptor);
}
__permissionStatusDefine(PermissionStatus.prototype, "constructor", {
  value: PermissionStatus,
  writable: true,
  configurable: true,
});
__permissionStatusDefine(PermissionStatus.prototype, Symbol.toStringTag, {
  value: "PermissionStatus",
  configurable: true,
});
globalThis.PermissionStatus = PermissionStatus;
const __createPermissionStatus = (name, baseState) => {
  const status = new __permissionStatusTarget();
  __permissionStatusSetPrototype(status, PermissionStatus.prototype);
  const labels = {
    __proto__: null,
    camera: "video_capture",
    push: "notifications",
    "persistent-storage": "durable_storage",
    "background-sync": "background_sync",
    "background-fetch": "background_fetch",
    "display-capture": "display_capture",
    "idle-detection": "idle_detection",
    "local-fonts": "local_fonts",
    "payment-handler": "payment_handler",
    "periodic-background-sync": "periodic_background_sync",
    "screen-wake-lock": "screen_wake_lock",
    accelerometer: "sensors",
    gyroscope: "sensors",
    magnetometer: "sensors",
    microphone: "audio_capture",
    "clipboard-read": "clipboard_read",
    "clipboard-write": "clipboard_write",
  };
  const state = {
    __proto__: null,
    document: __document,
    feature: name,
    name: labels[name] || name,
    baseState,
    lastState: baseState,
    handler: null,
    listener: null,
  };
  __permissionStatusRecords.set(status, state);
  state.notifiedState = __permissionStatusState(state);
  __permissionStatusAdd(new __permissionStatusWeakRef(status));
  return status;
};

const __refreshPermissionStatus = (reference) => {
  const status = __permissionStatusApply(__permissionStatusDereference, reference, []);
  if (!status) {
    __permissionStatusDelete(reference);
    return 0;
  }
  const state = __permissionStatusRecord(status),
    next = __permissionStatusState(state);
  if (next === state.notifiedState) return 0;
  state.notifiedState = next;
  __queueTimerTask(
    () => {
      if (!ops.op_window_active(state.document)) return;
      const event = new __hostEvent("change", { __proto__: null });
      ops.op_event_trust(event, -1);
      ops.op_dispatch_host_event(event, status);
    },
    __timerNow(),
    ops.op_task_sequence(),
  );
  return 1;
};
const __refreshPermissionStatuses = () => {
  let changed = 0;
  __permissionStatusEach((reference) => {
    changed += __refreshPermissionStatus(reference);
  });
  return changed;
};
