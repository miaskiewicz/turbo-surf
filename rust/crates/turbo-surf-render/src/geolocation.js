const __geoMap = new Map(),
  __geoOwners = __interfaceMap("Geolocation");
const __geoMapSize = Object.getOwnPropertyDescriptor(Map.prototype, "size").get.bind(__geoMap);
const __geoMapGet = Map.prototype.get.bind(__geoMap),
  __geoMapSet = Map.prototype.set.bind(__geoMap),
  __geoMapDelete = Map.prototype.delete.bind(__geoMap),
  __geoMapEach = Map.prototype.forEach.bind(__geoMap);
const __geoApply = Reflect.apply,
  __geoNumber = (value) => +value,
  __geoNow = Date.now.bind(Date),
  __geoTypeError = TypeError,
  __geoDefine = Object.defineProperty,
  __geoCreate = Object.create;
const __geoPositionRecords = __interfaceMap("GeolocationPosition"),
  __geoCoordinateRecords = __interfaceMap("GeolocationCoordinates"),
  __geoErrorRecords = __interfaceMap("GeolocationPositionError");
let __geoNextId = 1;
const __geoQueue = (callback) => __queueTimerTask(callback, __timerNow(), ops.op_task_sequence());
const __geoState = () => ops.op_context_emulation(__document, "geolocation");
const __geoBrand = (receiver) => {
  const owner = __geoOwners.get(receiver);
  if (!owner) throw new __geoTypeError("Illegal invocation");
  return owner;
};
const __geoRead = (records, receiver, field) => {
  const record = records.get(receiver);
  if (!record) throw new __geoTypeError("Illegal invocation");
  return record[field];
};
class GeolocationCoordinates {
  constructor() {
    throw new __geoTypeError("Illegal constructor");
  }
}
class GeolocationPosition {
  constructor() {
    throw new __geoTypeError("Illegal constructor");
  }
}
class GeolocationPositionError {
  constructor() {
    throw new __geoTypeError("Illegal constructor");
  }
}
const __geoInterface = (type, records, fields) => {
  for (const field of fields)
    __geoDefine(type.prototype, field, {
      get: __documentFunction(
        function () {
          return __geoRead(records, this, field);
        },
        "get " + field,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  __geoDefine(type.prototype, Symbol.toStringTag, { value: type.name, configurable: true });
};
__geoInterface(GeolocationCoordinates, __geoCoordinateRecords, [
  "latitude",
  "longitude",
  "altitude",
  "accuracy",
  "altitudeAccuracy",
  "heading",
  "speed",
]);
__geoInterface(GeolocationPosition, __geoPositionRecords, ["coords", "timestamp"]);
__geoInterface(GeolocationPositionError, __geoErrorRecords, ["code", "message"]);
for (const [name, code] of [
  ["PERMISSION_DENIED", 1],
  ["POSITION_UNAVAILABLE", 2],
  ["TIMEOUT", 3],
]) {
  for (const target of [GeolocationPositionError, GeolocationPositionError.prototype])
    __geoDefine(target, name, { value: code, enumerable: true });
}
const __geoPosition = (coordinates) => {
  const coords = __geoCreate(GeolocationCoordinates.prototype);
  __geoCoordinateRecords.set(coords, {
    __proto__: null,
    ...coordinates,
    altitude: null,
    altitudeAccuracy: null,
    heading: null,
    speed: null,
  });
  const position = __geoCreate(GeolocationPosition.prototype);
  __geoPositionRecords.set(position, { __proto__: null, coords, timestamp: __geoNow() });
  return position;
};
const __geoError = (code) => {
  const error = __geoCreate(GeolocationPositionError.prototype);
  __geoErrorRecords.set(error, {
    __proto__: null,
    code,
    message: [
      "",
      "Geolocation permission denied",
      "Position unavailable",
      "Position acquisition timed out",
    ][code],
  });
  return error;
};
const __geoOptions = (options) => {
  if (options == null) return { timeout: 4294967295 };
  if (typeof options !== "object" && typeof options !== "function")
    throw new __geoTypeError("Position options must be a dictionary");
  void !!options.enableHighAccuracy;
  const maximumAge = options.maximumAge;
  if (maximumAge !== undefined) __geoNumber(maximumAge);
  const raw = options.timeout;
  const timeout = raw === undefined ? 4294967295 : __geoNumber(raw);
  return { timeout: timeout > 0 ? timeout : 0 };
};
const __geoRequest = (success, error, options, watch) => {
  if (typeof success !== "function")
    throw new __geoTypeError("The success callback must be a function");
  if (error != null && typeof error !== "function")
    throw new __geoTypeError("The error callback must be a function");
  const config = __geoOptions(options);
  const id = __geoNextId++;
  const request = {
    __proto__: null,
    id,
    success,
    error,
    watch,
    timeout: config.timeout,
    last: null,
    queued: false,
  };
  __geoMapSet(id, request);
  __geoSchedule(request, __geoState());
  return watch ? id : undefined;
};
const __geoResultCode = (state, request) => {
  if (state.permission !== "granted") return 1;
  if (request.timeout === 0) return 3;
  return state.position ? 0 : 2;
};
const __geoDeliver = (request, state) => {
  request.queued = false;
  if (__geoMapGet(request.id) !== request) return;
  const current = __geoState();
  if (!current.active) {
    __geoMapDelete(request.id);
    return;
  }
  if (current.permission !== "granted") state = current;
  const code = __geoResultCode(state, request);
  if (!request.watch || code === 1) __geoMapDelete(request.id);
  const callback = code ? request.error : request.success;
  if (callback)
    __geoApply(callback, undefined, [code ? __geoError(code) : __geoPosition(state.position)]);
};
const __geoSchedule = (request, state) => {
  const position = state.position;
  const key =
    state.active +
    "|" +
    state.permission +
    "|" +
    (position
      ? position.latitude + "|" + position.longitude + "|" + position.accuracy
      : "unavailable");
  if (request.last === key || request.queued) return 0;
  request.last = key;
  request.queued = true;
  __geoQueue(() => __geoDeliver(request, state));
  return 1;
};
class Geolocation {
  constructor() {
    throw new __geoTypeError("Illegal constructor");
  }
  getCurrentPosition(success, error, options) {
    return __geoBrand(this).request(success, error, options, false);
  }
  watchPosition(success, error, options) {
    return __geoBrand(this).request(success, error, options, true);
  }
  clearWatch(id) {
    __geoBrand(this).clear(id);
  }
}
for (const [name, length] of [
  ["getCurrentPosition", 1],
  ["watchPosition", 1],
  ["clearWatch", 1],
])
  __geoDefine(Geolocation.prototype, name, {
    value: __documentFunction(Geolocation.prototype[name], name, length),
    writable: true,
    configurable: true,
    enumerable: true,
  });
__geoDefine(Geolocation.prototype, Symbol.toStringTag, {
  value: "Geolocation",
  configurable: true,
});
const __geolocation = __geoCreate(Geolocation.prototype);
__geoOwners.set(__geolocation, {
  __proto__: null,
  request: __geoRequest,
  clear: (id) => __geoMapDelete(__geoNumber(id) | 0),
});
__geoDefine(Navigator.prototype, "geolocation", {
  get: __documentFunction(
    function () {
      // The Navigator getter retains the native interface's existing receiver brand check.
      if (this !== navigator) throw new __geoTypeError("Illegal invocation");
      return __geolocation;
    },
    "get geolocation",
    0,
  ),
  configurable: true,
  enumerable: true,
});
Object.assign(globalThis, {
  Geolocation,
  GeolocationCoordinates,
  GeolocationPosition,
  GeolocationPositionError,
});
const __deliverEmulationState = () => {
  let changed = __refreshPermissionStatuses();
  if (!__geoMapSize() || !ops.op_window_active(__document)) return changed;
  const state = __geoState();
  __geoMapEach((request) => {
    if (request.watch) changed += __geoSchedule(request, state);
  });
  return changed;
};
ops.op_set_host_capabilities({
  deliverEmulationState: __deliverEmulationState,
  hasEmulationObservers: () => __geoMapSize() > 0 || __permissionStatusSize() > 0,
});
