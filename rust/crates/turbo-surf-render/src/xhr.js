// XMLHttpRequest, XMLHttpRequestEventTarget and XMLHttpRequestUpload (#28,
// harness/detection/transports-probe.js) over the engine's fetch: the XHR state machine,
// argument validation, header lists, response types and Chrome's trusted event order
// (readystatechange 2 and 3, progress, readystatechange 4, load, loadend).
// responseXML and responseType "document" parse XML MIME types with turbo-dom-browser-env's
// XML parser (turbo-dom#186; ill-formed XML is null, as in Chrome) and, for "document",
// text/html with the HTML parser. Synchronous requests (async=false, turbo-surf#138) block
// in send() over op_fetch_sync with Chrome 156's window restrictions
// (harness/detection/sync-xhr-probe.js). Not modelled: upload progress events and the
// sync-XHR deprecation console message. setAttributionReporting and
// setPrivateToken validate as Chrome 156 does (turbo-surf#165) and request nothing:
// attribution reporting and Private State Token operations are not modelled.
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
    split = String.prototype.split,
    endsWith = String.prototype.endsWith,
    fromCharCode = String.fromCharCode,
    trim = String.prototype.trim,
    replace = String.prototype.replace;
  const stringify = JSON.stringify,
    startsWith = String.prototype.startsWith;
  const arrayPush = Array.prototype.push,
    arrayIncludes = Array.prototype.includes,
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
        sync: false,
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
        document: undefined,
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
  // The XHR "document response" (Chrome 156: harness/detection/xhr-document-probe.js).
  const documentResponse = (record) => {
    if (record.state !== 4) return null;
    if (record.document !== undefined) return record.document;
    const essence = apply(
      trim,
      apply(lower, apply(split, finalMime(record) ?? "text/xml", [";"])[0], []),
      [],
    );
    let doc = null;
    if (essence === "text/html") {
      if (record.responseType === "document")
        doc = __registerInertDocument(
          ops.op_parse_html_document(text(record), false),
          record.response.url,
          "text/html",
        );
    } else if (isXmlEssence(essence)) {
      // Bounded by the network body cap (8 MiB); checked again for other sources.
      if (record.bytes && record.bytes.length <= MAX_DOCUMENT_BYTES) {
        // null when not well-formed (turbo-dom's ParsedXmlDocument::well_formed).
        const parsed = ops.op_parse_xml_response(text(record), essence);
        if (parsed !== null) doc = __registerInertDocument(parsed, record.response.url, essence);
      }
    }
    record.document = doc;
    return doc;
  };
  const MAX_DOCUMENT_BYTES = 8 * 1024 * 1024;
  const isXmlEssence = (essence) =>
    essence === "text/xml" || essence === "application/xml" || apply(endsWith, essence, ["+xml"]);
  // A byte order mark decides the encoding first.
  const bomLabel = (bytes) =>
    bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf
      ? "utf-8"
      : bytes[0] === 0xfe && bytes[1] === 0xff
        ? "utf-16be"
        : bytes[0] === 0xff && bytes[1] === 0xfe
          ? "utf-16le"
          : null;
  // The `<?xml ... encoding="…"?>` declaration, read from the ASCII prefix.
  const declaredLabel = (bytes) => {
    let prefix = "";
    for (let i = 0; i < bytes.length && i < 1024; i++) {
      if (bytes[i] > 0x7f) break;
      prefix += apply(fromCharCode, null, [bytes[i]]);
      if (bytes[i] === 0x3e) break;
    }
    const match = apply(exec, /^<\?xml\s[^>]*?\bencoding\s*=\s*["']([A-Za-z][A-Za-z0-9._-]*)["']/, [
      prefix,
    ]);
    if (!match) return null;
    // An ASCII-readable declaration cannot be UTF-16: sniffing maps it to UTF-8.
    const label = apply(lower, match[1], []);
    return label === "utf-16" || label === "utf-16le" || label === "utf-16be" ? "utf-8" : match[1];
  };
  // XHR text decoding: BOM, then the final MIME type's charset, then (XML types, as
  // the XML parser sniffs) the declaration, then UTF-8 (Chrome 156 control).
  const text = (record) => {
    if (!record.bytes) return "";
    const mime = finalMime(record);
    const essence = apply(trim, apply(lower, apply(split, mime ?? "text/xml", [";"])[0], []), []);
    const labels = [bomLabel(record.bytes), charset(mime)];
    if (isXmlEssence(essence)) apply(arrayPush, labels, [declaredLabel(record.bytes)]);
    for (let i = 0; i < labels.length; i++)
      if (labels[i])
        try {
          return apply(decode, new Decoder(labels[i]), [record.bytes]);
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
    record.document = undefined;
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

  // Chrome 156 rejects these at open() for a document's synchronous request.
  const synchronousOpen = (record) => {
    if (record.responseType !== "")
      throw failed(
        "open",
        "Synchronous requests from a document must not set a response type.",
        "InvalidAccessError",
      );
    if (record.timeout)
      throw failed("open", "Synchronous requests must not set a timeout.", "InvalidAccessError");
  };
  // The request body as the async path sends it, read synchronously.
  const syncBody = (init, body) => {
    if (body === null || body === undefined) return;
    if (typeof body === "string") init.body = body;
    else if (body instanceof ArrayBuffer) init.body_bytes = Array.from(new Uint8Array(body));
    else if (ArrayBuffer.isView(body))
      init.body_bytes = Array.from(new Uint8Array(body.buffer, body.byteOffset, body.byteLength));
    else if (body instanceof BlobValue) init.body_bytes = Array.from(__readBlobBytes(body));
    else init.body = string(body);
  };
  // The fields __fetchFromSource marshals for the page's own fetch op.
  const syncInit = (record, body) => {
    const headers = {};
    for (const [name, value] of record.headers) headers[name] = value;
    const init = {
      ...ops.op_document_cookie_site(__document),
      method: record.method,
      headers,
      credentials: record.credentials ? "include" : "same-origin",
      initiator: "xmlhttprequest",
      source: ops.op_document_request_source(__document),
      referrer_source: record.source(),
      opaque_origin: ops.op_window_opaque(__document),
      blob_key: apply(startsWith, record.url, ["blob:"])
        ? ops.op_blob_url_key(__document)
        : undefined,
      csp: ops.op_csp_context(__document),
    };
    syncBody(init, body);
    return apply(stringify, JSON, [init]);
  };
  const syncFailure = (record) =>
    failed(
      "send",
      `Failed to load '${record.url}'${
        apply(startsWith, record.url, ["blob:"]) && record.method !== "GET"
          ? ": 'GET' is the only method allowed for 'blob:' URLs"
          : ""
      }.`,
      "NetworkError",
    );
  const responseRecord = (record, result) => {
    const list = [];
    let typed = false;
    for (const [name, value] of result.headers || []) {
      const key = apply(lower, name, []);
      if (key === "content-type") typed = true;
      if (key !== "set-cookie" && key !== "set-cookie2") apply(arrayPush, list, [[key, value]]);
    }
    if (result.content_type && !typed)
      apply(arrayPush, list, [["content-type", result.content_type]]);
    return {
      status: result.status,
      statusText: result.status_text || "",
      url: apply(replace, result.final_url || record.url, [/#.*$/, ""]),
      headers: list,
    };
  };
  // A synchronous send fires no loadstart or progress; failures throw instead of
  // firing error events (Chrome 156 control).
  const sendSynchronously = (request, record, body) => {
    const generation = record.generation;
    const result = ops.op_csp_request(__document, record.url, "connect-src", "")
      ? ops.op_fetch_sync(record.url, syncInit(record, body))
      : { status: 0 };
    if (record.generation !== generation) return;
    if (!result.status) {
      reset(record);
      record.state = 4;
      throw syncFailure(record);
    }
    record.response = responseRecord(record, result);
    record.bytes = new Uint8Array(result.body_bytes || []);
    const size = record.bytes.length;
    changeState(request, record, 4);
    fire(record, request, "load", size, size, true);
    fire(record, request, "loadend", size, size, true);
  };

  getter("readyState", (record) => record.state);
  getter(
    "timeout",
    (record) => record.timeout,
    (record, value) => {
      value = NumberValue(value) >>> 0;
      if (record.sync)
        throw new Exception(
          "Failed to set the 'timeout' property on 'XMLHttpRequest': Timeouts cannot be set for synchronous requests made from a document.",
          "InvalidAccessError",
        );
      record.timeout = value;
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
      if (record.sync)
        throw new Exception(
          "Failed to set the 'responseType' property on 'XMLHttpRequest': The response type cannot be changed for synchronous requests made from a document.",
          "InvalidAccessError",
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
    } else record.cache = documentResponse(record);
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
  method("open", 2, function (verb, address, async, username, password) {
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
    // Web IDL: an explicit third argument converts to boolean, undefined included.
    const synchronous = arguments.length > 2 && !async;
    reset(record);
    // Chrome aborts and resets to UNSENT before the synchronous checks can throw.
    const previous = record.state;
    record.state = 0;
    record.send = false;
    if (synchronous) synchronousOpen(record);
    record.sync = synchronous;
    record.method = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"].includes(normalized)
      ? normalized
      : verb;
    record.url = parsed.href;
    record.headers = [];
    if (previous !== 1) changeState(this, record, 1);
    else record.state = 1;
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
    if (record.sync) return sendSynchronously(this, record, body);
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
      initiator: "xmlhttprequest",
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
    return documentResponse(record);
  });
  const required = (name, count) => {
    if (count < 1)
      throw new ErrorType(
        `Failed to execute '${name}' on 'XMLHttpRequest': 1 argument required, but only 0 present.`,
      );
  };
  // Chrome 156 reads no member of the options dictionary.
  method("setAttributionReporting", 1, function (_options) {
    state(this);
    required("setAttributionReporting", arguments.length);
  });
  const tokenFailure = (message) =>
    new ErrorType(`Failed to execute 'setPrivateToken' on 'XMLHttpRequest': ${message}`);
  const tokenMember = (message) => tokenFailure(`Failed to read the ${message}`);
  const tokenEnum = (dictionary, name, type, values, needed) => {
    const value = dictionary[name];
    if (value === undefined) {
      if (needed)
        throw tokenMember(`'${name}' property from 'PrivateToken': Required member is undefined.`);
      return undefined;
    }
    const text = string(value);
    if (!apply(arrayIncludes, values, [text]))
      throw tokenMember(
        `'${name}' property from 'PrivateToken': The provided value '${text}' is not a valid enum value of type ${type}.`,
      );
    return text;
  };
  const issuerList = (value) => {
    if (value === undefined) return undefined;
    const iterator =
      value !== null && (typeof value === "object" || typeof value === "function")
        ? value[Symbol.iterator]
        : undefined;
    if (typeof iterator !== "function")
      throw tokenMember(
        "'issuers' property from 'PrivateToken': The provided value cannot be converted to a sequence.",
      );
    const list = [];
    for (const item of { [Symbol.iterator]: () => apply(iterator, value, []) })
      apply(arrayPush, list, [string(item)]);
    return list;
  };
  // Potentially trustworthy as Chrome judges an issuer: https, or http on loopback.
  const secureIssuer = (url) =>
    url.protocol === "https:" ||
    url.hostname === "localhost" ||
    apply(endsWith, url.hostname, [".localhost"]) ||
    url.hostname === "[::1]" ||
    apply(exec, /^127(?:\.\d{1,3}){3}$/, [url.hostname]) !== null;
  method("setPrivateToken", 1, function (privateToken) {
    const record = state(this);
    required("setPrivateToken", arguments.length);
    if (
      privateToken === null ||
      (typeof privateToken !== "object" && typeof privateToken !== "function")
    )
      throw tokenFailure("The provided value is not of type 'PrivateToken'.");
    // Dictionary members convert in lexicographic order.
    const issuers = issuerList(privateToken.issuers);
    const operation = tokenEnum(
      privateToken,
      "operation",
      "OperationType",
      ["token-request", "send-redemption-record", "token-redemption"],
      true,
    );
    tokenEnum(privateToken, "refreshPolicy", "RefreshPolicy", ["none", "refresh"], false);
    tokenEnum(privateToken, "version", "PrivateTokenVersion", ["1"], true);
    if (record.state !== 1 || record.send)
      throw failed("setPrivateToken", "The object's state must be OPENED.", "InvalidStateError");
    if (operation !== "send-redemption-record") return;
    if (!issuers || !issuers.length)
      throw tokenFailure(
        "privateToken: operation type 'send-redemption-record' requires that the 'issuers' field be present and contain at least one secure, HTTP(S) URL, but it was missing or empty.",
      );
    for (const issuer of issuers) {
      let url = null;
      try {
        url = new LocationURL(issuer);
      } catch {}
      if (!url || (url.protocol !== "https:" && url.protocol !== "http:"))
        throw tokenFailure(
          `privateToken: operation type 'send-redemption-record' requires that the 'issuers' fields' members parse to HTTP(S) origins, but one did not: ${issuer}`,
        );
      if (!secureIssuer(url))
        throw tokenFailure(
          `privateToken: operation type 'send-redemption-record' requires that the 'issuers' fields' members parse to secure origins, but one did not: ${issuer}`,
        );
    }
  });
})();
