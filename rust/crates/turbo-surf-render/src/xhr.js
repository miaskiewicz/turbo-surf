// XMLHttpRequest, XMLHttpRequestEventTarget and XMLHttpRequestUpload (#28,
// harness/detection/transports-probe.js) over the engine's fetch: the XHR state machine,
// argument validation, header lists, response types and Chrome's trusted event order
// (readystatechange 2 and 3, progress, readystatechange 4, load, loadend).
// Not modelled: synchronous requests (async=false still completes asynchronously),
// responseXML/"document" parsing (null), upload progress events, setAttributionReporting
// and setPrivateToken.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    Progress = globalThis.ProgressEvent,
    PlainEvent = __hostEvent;
  const LocationURL = globalThis.URL,
    StringValue = String,
    NumberValue = Number,
    BlobValue = globalThis.Blob;
  const Decoder = globalThis.TextDecoder,
    decode = Decoder.prototype.decode,
    parseJSON = JSON.parse;
  const Controller = globalThis.AbortController,
    abortController = Controller.prototype.abort;
  const hostTimeout = globalThis.setTimeout,
    hostClear = globalThis.clearTimeout;
  const exec = RegExp.prototype.exec,
    lower = String.prototype.toLowerCase,
    upper = String.prototype.toUpperCase,
    trim = String.prototype.trim,
    replace = String.prototype.replace;
  const arrayPush = Array.prototype.push,
    arraySort = Array.prototype.sort;
  const states = new WeakMap(),
    stateGet = WeakMap.prototype.get,
    stateSet = WeakMap.prototype.set;
  const lookup = (receiver) =>
    receiver !== null && typeof receiver === "object"
      ? apply(stateGet, states, [receiver])
      : undefined;
  const state = (receiver) => {
    const value = lookup(receiver);
    if (!value || value.kind !== "XMLHttpRequest") throw new ErrorType("Illegal invocation");
    return value;
  };
  const string = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
    return StringValue(value);
  };
  const failed = (operation, message, name) =>
    new Exception(`Failed to execute '${operation}' on 'XMLHttpRequest': ${message}`, name);
  const token = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const forbiddenHeaders = [
    "accept-charset",
    "accept-encoding",
    "access-control-request-headers",
    "access-control-request-method",
    "connection",
    "content-length",
    "cookie",
    "cookie2",
    "date",
    "dnt",
    "expect",
    "host",
    "keep-alive",
    "origin",
    "referer",
    "set-cookie",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "via",
  ];
  const forbiddenHeader = (name) =>
    forbiddenHeaders.includes(name) || name.startsWith("proxy-") || name.startsWith("sec-");

  const EventTargetBase = __defineInterface("XMLHttpRequestEventTarget", 0, EventTarget);
  const checkTarget = (receiver) => {
    if (!lookup(receiver)) throw new ErrorType("Illegal invocation");
  };
  for (const name of [
    "onloadstart",
    "onprogress",
    "onabort",
    "onerror",
    "onload",
    "ontimeout",
    "onloadend",
  ])
    __defineEventHandler(EventTargetBase.prototype, name, checkTarget);
  const Upload = __defineInterface("XMLHttpRequestUpload", 0, EventTargetBase);
  const target = (prototype) => {
    const object = new EventTarget();
    Object.setPrototypeOf(object, prototype);
    return object;
  };

  const XMLHttpRequest = __defineInterface("XMLHttpRequest", 0, EventTargetBase, function () {
    if (!new.target)
      throw new ErrorType(
        "Failed to construct 'XMLHttpRequest': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
      );
    const request = target(new.target.prototype),
      upload = target(Upload.prototype);
    apply(stateSet, states, [upload, { __proto__: null, kind: "XMLHttpRequestUpload" }]);
    apply(stateSet, states, [
      request,
      {
        __proto__: null,
        kind: "XMLHttpRequest",
        state: 0,
        send: false,
        method: "GET",
        url: null,
        headers: [],
        timeout: 0,
        credentials: false,
        responseType: "",
        upload,
        override: null,
        generation: 0,
        response: null,
        bytes: null,
        cache: undefined,
        controller: null,
        timer: null,
        source: () => ops.op_document_request_url(),
        base: () => apply(__baseURI, __document, []),
      },
    ]);
    return request;
  });
  const P = XMLHttpRequest.prototype;
  const getter = (name, read, write) =>
    define(P, name, {
      __proto__: null,
      get: make(
        function () {
          return read(state(this));
        },
        "get " + name,
        0,
      ),
      set: write
        ? make(
            function (value) {
              write(state(this), value);
            },
            "set " + name,
            1,
          )
        : undefined,
      enumerable: true,
      configurable: true,
    });
  __defineEventHandler(P, "onreadystatechange", (receiver) => void state(receiver));
  const fire = (record, object, type, loaded = 0, total = 0, computable = false) =>
    __dispatchTrusted(
      object,
      type === "readystatechange"
        ? new PlainEvent(type)
        : new Progress(type, { lengthComputable: computable, loaded, total }),
    );
  const changeState = (request, record, value) => {
    record.state = value;
    fire(record, request, "readystatechange");
  };
  // Header lists keep insertion order; reads sort and combine like Fetch.
  const headerValue = (list, name) => {
    let value = null;
    for (const [key, entry] of list)
      if (key === name) value = value === null ? entry : value + ", " + entry;
    return value;
  };
  const charset = (mime) => {
    const match = mime && apply(exec, /;\s*charset\s*=\s*"?([^";]+)/i, [mime]);
    return match ? apply(trim, match[1], []) : null;
  };
  const finalMime = (record) =>
    record.override ??
    (record.response ? headerValue(record.response.headers, "content-type") : null);
  const text = (record) => {
    if (!record.bytes) return "";
    const label = charset(finalMime(record));
    if (label)
      try {
        return apply(decode, new Decoder(label), [record.bytes]);
      } catch {}
    return apply(decode, new Decoder(), [record.bytes]);
  };
  const reset = (record) => {
    record.generation++;
    if (record.controller) apply(abortController, record.controller, []);
    if (record.timer !== null) apply(hostClear, globalThis, [record.timer]);
    record.controller = null;
    record.timer = null;
    record.response = null;
    record.bytes = null;
    record.cache = undefined;
  };
  // XHR "request error steps": DONE, then the error event and loadend.
  const requestError = (request, record, type) => {
    reset(record);
    record.send = false;
    changeState(request, record, 4);
    fire(record, request, type);
    fire(record, request, "loadend");
  };

  getter("readyState", (record) => record.state);
  getter(
    "timeout",
    (record) => record.timeout,
    (record, value) => {
      record.timeout = NumberValue(value) >>> 0;
    },
  );
  getter(
    "withCredentials",
    (record) => record.credentials,
    (record, value) => {
      if ((record.state !== 0 && record.state !== 1) || record.send)
        throw new Exception(
          "Failed to set the 'withCredentials' property on 'XMLHttpRequest': The value may only be set if the object's state is UNSENT or OPENED.",
          "InvalidStateError",
        );
      record.credentials = !!value;
    },
  );
  getter("upload", (record) => record.upload);
  getter("responseURL", (record) => (record.response ? record.response.url : ""));
  getter("status", (record) => (record.response ? record.response.status : 0));
  getter("statusText", (record) => (record.response ? record.response.statusText : ""));
  getter(
    "responseType",
    (record) => record.responseType,
    (record, value) => {
      value = string(value);
      if (!["", "arraybuffer", "blob", "document", "json", "text"].includes(value)) return;
      if (record.state === 3 || record.state === 4)
        throw new Exception(
          "Failed to set the 'responseType' property on 'XMLHttpRequest': The response type cannot be set if the object's state is LOADING or DONE.",
          "InvalidStateError",
        );
      record.responseType = value;
    },
  );
  getter("response", (record) => {
    if (record.responseType === "" || record.responseType === "text")
      return record.state === 3 || record.state === 4 ? text(record) : "";
    if (record.state !== 4) return null;
    if (record.cache !== undefined) return record.cache;
    const bytes = record.bytes || new Uint8Array();
    if (record.responseType === "arraybuffer") record.cache = bytes.slice().buffer;
    else if (record.responseType === "blob")
      record.cache = new BlobValue([bytes], { type: finalMime(record) || "" });
    else if (record.responseType === "json") {
      try {
        record.cache = apply(parseJSON, JSON, [apply(decode, new Decoder(), [bytes])]);
      } catch {
        record.cache = null;
      }
    } else record.cache = null;
    return record.cache;
  });
  getter("responseText", (record) => {
    if (record.responseType !== "" && record.responseType !== "text")
      throw new Exception(
        `Failed to read the 'responseText' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'text' (was '${record.responseType}').`,
        "InvalidStateError",
      );
    return record.state === 3 || record.state === 4 ? text(record) : "";
  });
  __defineConstants(XMLHttpRequest, [
    ["UNSENT", 0],
    ["OPENED", 1],
    ["HEADERS_RECEIVED", 2],
    ["LOADING", 3],
    ["DONE", 4],
  ]);
  const method = (name, length, implementation) =>
    define(P, name, {
      __proto__: null,
      value: make(implementation, name, length),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  method("abort", 0, function () {
    const record = state(this);
    if ((record.state === 1 && record.send) || record.state === 2 || record.state === 3)
      requestError(this, record, "abort");
    if (record.state === 4) {
      record.state = 0;
      reset(record);
    }
  });
  method("getAllResponseHeaders", 0, function () {
    const record = state(this);
    if (!record.response) return "";
    const names = [];
    for (const [name] of record.response.headers)
      if (!names.includes(name)) apply(arrayPush, names, [name]);
    apply(arraySort, names, []);
    let output = "";
    for (const name of names)
      output += name + ": " + headerValue(record.response.headers, name) + "\r\n";
    return output;
  });
  method("getResponseHeader", 1, function (name) {
    const record = state(this);
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'getResponseHeader' on 'XMLHttpRequest': 1 argument required, but only 0 present.",
      );
    name = apply(lower, string(name), []);
    return record.response ? headerValue(record.response.headers, name) : null;
  });
  method("open", 2, function (verb, address, async = true, username = null, password = null) {
    const record = state(this);
    if (arguments.length < 2)
      throw new ErrorType(
        `Failed to execute 'open' on 'XMLHttpRequest': 2 arguments required, but only ${arguments.length} present.`,
      );
    verb = string(verb);
    address = string(address);
    if (!apply(exec, token, [verb]))
      throw failed("open", `'${verb}' is not a valid HTTP method.`, "SyntaxError");
    const normalized = apply(upper, verb, []);
    if (normalized === "CONNECT" || normalized === "TRACE" || normalized === "TRACK")
      throw failed("open", `'${verb}' HTTP method is unsupported.`, "SecurityError");
    let parsed;
    try {
      parsed = new LocationURL(address, record.base());
    } catch {
      throw failed("open", `Invalid URL`, "SyntaxError");
    }
    if (username !== null && username !== undefined) parsed.username = string(username);
    if (password !== null && password !== undefined) parsed.password = string(password);
    void async;
    reset(record);
    record.method = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"].includes(normalized)
      ? normalized
      : verb;
    record.url = parsed.href;
    record.headers = [];
    record.send = false;
    if (record.state !== 1) changeState(this, record, 1);
  });
  method("overrideMimeType", 1, function (mime) {
    const record = state(this);
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'overrideMimeType' on 'XMLHttpRequest': 1 argument required, but only 0 present.",
      );
    if (record.state === 3 || record.state === 4)
      throw failed(
        "overrideMimeType",
        "MimeType cannot be overridden when the state is LOADING or DONE.",
        "InvalidStateError",
      );
    mime = apply(trim, string(mime), []);
    record.override = apply(
      exec,
      /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+\/[!#$%&'*+\-.^_`|~0-9A-Za-z]+(\s*;.*)?$/,
      [mime],
    )
      ? mime
      : "application/octet-stream";
  });
  method("send", 0, function (body = null) {
    const record = state(this);
    if (record.state !== 1)
      throw failed("send", "The object's state must be OPENED.", "InvalidStateError");
    if (record.send)
      throw failed("send", "The object's state must be OPENED.", "InvalidStateError");
    if (record.method === "GET" || record.method === "HEAD") body = null;
    record.send = true;
    const request = this,
      generation = record.generation;
    fire(record, request, "loadstart");
    const headers = {};
    for (const [name, value] of record.headers) headers[name] = value;
    record.controller = new Controller();
    const live = () => record.generation === generation;
    if (record.timeout)
      record.timer = apply(hostTimeout, globalThis, [
        () => {
          if (live()) requestError(request, record, "timeout");
        },
        record.timeout,
      ]);
    const init = {
      method: record.method,
      headers,
      credentials: record.credentials ? "include" : "same-origin",
      signal: record.controller.signal,
    };
    if (body !== null && body !== undefined) init.body = body;
    __fetchFromSource(record.url, init, record.source())
      .then(async (response) => {
        if (!live()) return;
        const list = [];
        for (const [name, value] of response.headers) {
          const key = apply(lower, name, []);
          if (key !== "set-cookie" && key !== "set-cookie2") apply(arrayPush, list, [[key, value]]);
        }
        record.response = {
          status: response.status,
          statusText: response.statusText || "",
          url: apply(replace, response.url || record.url, [/#.*$/, ""]),
          headers: list,
        };
        changeState(request, record, 2);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (!live()) return;
        const length = headerValue(list, "content-length"),
          total = length !== null && /^\d+$/.test(length) ? NumberValue(length) : 0;
        record.bytes = bytes;
        changeState(request, record, 3);
        fire(record, request, "progress", bytes.length, total, length !== null);
        if (record.timer !== null) apply(hostClear, globalThis, [record.timer]);
        record.timer = null;
        record.send = false;
        changeState(request, record, 4);
        fire(record, request, "load", bytes.length, total, length !== null);
        fire(record, request, "loadend", bytes.length, total, length !== null);
      })
      .catch(() => {
        if (live()) requestError(request, record, "error");
      });
  });
  method("setRequestHeader", 2, function (name, value) {
    const record = state(this);
    if (arguments.length < 2)
      throw new ErrorType(
        `Failed to execute 'setRequestHeader' on 'XMLHttpRequest': 2 arguments required, but only ${arguments.length} present.`,
      );
    name = string(name);
    value = apply(replace, string(value), [/^[\t\n\r ]+|[\t\n\r ]+$/g, ""]);
    if (record.state !== 1)
      throw failed("setRequestHeader", "The object's state must be OPENED.", "InvalidStateError");
    if (record.send)
      throw failed("setRequestHeader", "The object's state must be OPENED.", "InvalidStateError");
    if (!apply(exec, token, [name]))
      throw failed(
        "setRequestHeader",
        `'${name}' is not a valid HTTP header field name.`,
        "SyntaxError",
      );
    if (apply(exec, /[\0\r\n]/, [value]))
      throw failed(
        "setRequestHeader",
        `'${value}' is not a valid HTTP header field value.`,
        "SyntaxError",
      );
    const key = apply(lower, name, []);
    if (forbiddenHeader(key)) return;
    for (const entry of record.headers)
      if (entry[0] === key) {
        entry[1] += ", " + value;
        return;
      }
    apply(arrayPush, record.headers, [[key, value]]);
  });
  getter("responseXML", (record) => {
    if (record.responseType !== "" && record.responseType !== "document")
      throw new Exception(
        `Failed to read the 'responseXML' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'document' (was '${record.responseType}').`,
        "InvalidStateError",
      );
    return null;
  });
})();
