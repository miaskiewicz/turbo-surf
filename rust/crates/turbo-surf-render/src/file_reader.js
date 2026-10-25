// FileReader (#28, harness/detection/transports-probe.js): the File API read operation over a
// Blob's private bytes — loadstart, one progress, load and loadend as trusted ProgressEvents in
// queued tasks; abort() fires abort and loadend synchronously and cancels the pending tasks.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    Progress = globalThis.ProgressEvent;
  const Decoder = globalThis.TextDecoder,
    decode = Decoder.prototype.decode,
    hostTimeout = globalThis.setTimeout;
  const fromCharCode = String.fromCharCode,
    StringValue = String,
    base64 = globalThis.btoa;
  const states = new WeakMap(),
    stateGet = WeakMap.prototype.get,
    stateSet = WeakMap.prototype.set;
  const state = (receiver) => {
    const value =
      receiver !== null && typeof receiver === "object"
        ? apply(stateGet, states, [receiver])
        : undefined;
    if (!value) throw new ErrorType("Illegal invocation");
    return value;
  };
  const check = (receiver) => void state(receiver);
  const FileReader = __defineInterface("FileReader", 0, EventTarget, function () {
    if (!new.target)
      throw new ErrorType(
        "Failed to construct 'FileReader': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
      );
    const target = new EventTarget();
    Object.setPrototypeOf(target, new.target.prototype);
    apply(stateSet, states, [
      target,
      { __proto__: null, readyState: 0, result: null, error: null, generation: 0 },
    ]);
    return target;
  });
  const P = FileReader.prototype;
  __defineConstants(FileReader, [
    ["EMPTY", 0],
    ["LOADING", 1],
    ["DONE", 2],
  ]);
  for (const name of ["readyState", "result", "error"])
    define(P, name, {
      __proto__: null,
      get: make(
        function () {
          return state(this)[name];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  for (const name of ["onloadstart", "onprogress", "onload", "onabort", "onerror", "onloadend"])
    __defineEventHandler(P, name, check);
  const fire = (reader, type, loaded, total, lengthComputable = true) =>
    __dispatchTrusted(reader, new Progress(type, { lengthComputable, loaded, total }));
  const binary = (bytes) => {
    let text = "";
    for (let at = 0; at < bytes.length; at += 8192)
      text += apply(fromCharCode, null, bytes.subarray(at, at + 8192));
    return text;
  };
  // Encoding as Chrome 156 applies it: an exact (untrimmed) label naming an encoding, else UTF-8
  // (the blob type's charset is not consulted); a byte order mark wins over both.
  const replacementLabels = [
    "csiso2022kr",
    "hz-gb-2312",
    "iso-2022-cn",
    "iso-2022-cn-ext",
    "iso-2022-kr",
    "replacement",
  ];
  const text = (bytes, label) => {
    let encoding = "utf-8";
    if (label !== undefined && label === apply(String.prototype.trim, label, [])) {
      if (replacementLabels.includes(apply(String.prototype.toLowerCase, label, [])))
        return bytes.length ? "\ufffd" : "";
      try {
        new Decoder(label);
        encoding = label;
      } catch {}
    }
    if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) encoding = "utf-8";
    else if (bytes[0] === 0xfe && bytes[1] === 0xff) encoding = "utf-16be";
    else if (bytes[0] === 0xff && bytes[1] === 0xfe) encoding = "utf-16le";
    return apply(decode, new Decoder(encoding), [bytes]);
  };
  const packages = {
    __proto__: null,
    readAsArrayBuffer: (bytes) => bytes.slice().buffer,
    readAsBinaryString: binary,
    readAsDataURL: (bytes, _label, type) =>
      "data:" + (type || "application/octet-stream") + ";base64," + base64(binary(bytes)),
    readAsText: text,
  };
  function startRead(reader, method, blob, label) {
    const record = state(reader);
    // Chrome records a readAsText encoding before refusing a busy reader, so a rejected
    // call still replaces the encoding of the read in progress.
    if (method === "readAsText") record.label = label;
    const blobState = __blobStates.get(blob);
    if (!blobState)
      throw new ErrorType(
        `Failed to execute '${method}' on 'FileReader': parameter 1 is not of type 'Blob'.`,
      );
    if (record.readyState === 1)
      throw new Exception("The object is already busy reading Blobs.", "InvalidStateError");
    const bytes = blobState.bytes,
      type = blobState.type,
      total = bytes.length;
    record.readyState = 1;
    record.result = null;
    record.error = null;
    const generation = ++record.generation;
    const live = () => record.generation === generation;
    const queue = (step) =>
      apply(hostTimeout, globalThis, [
        () => {
          if (live()) step();
        },
        0,
      ]);
    queue(() => {
      fire(reader, "loadstart", 0, total);
      queue(() => {
        if (total) fire(reader, "progress", total, total);
        queue(() => {
          record.readyState = 2;
          record.result = packages[method](bytes, record.label, type);
          fire(reader, "load", total, total);
          if (record.readyState !== 1) fire(reader, "loadend", total, total);
        });
      });
    });
  }
  for (const method of ["readAsArrayBuffer", "readAsBinaryString", "readAsDataURL", "readAsText"])
    define(P, method, {
      __proto__: null,
      value: make(
        function (blob, label = undefined) {
          check(this);
          if (!arguments.length)
            throw new ErrorType(
              `Failed to execute '${method}' on 'FileReader': 1 argument required, but only 0 present.`,
            );
          if (method === "readAsText" && label !== undefined) label = StringValue(label);
          startRead(this, method, blob, label);
        },
        method,
        1,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  define(P, "abort", {
    __proto__: null,
    value: make(
      function () {
        const record = state(this);
        if (record.readyState !== 1) {
          record.result = null;
          return;
        }
        record.readyState = 2;
        record.result = null;
        record.generation++;
        fire(this, "abort", 0, 0, false);
        if (record.readyState !== 1) fire(this, "loadend", 0, 0, false);
      },
      "abort",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
