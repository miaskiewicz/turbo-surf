/* eslint-disable no-underscore-dangle, no-await-in-loop, unicorn/consistent-function-scoping, no-shadow, no-unused-vars */
// EventSource (#144, network-transports-probe.js): the HTML event-stream processing model
// over the page's own fetch path (__fetchFromSource), so CSP connect-src, CORS, cookies,
// client hints and the request context are the ones every fetch gets. Reads are unref'd
// (an open stream never holds the page's event loop); a dropped stream reconnects after
// the stream's `retry` time with Last-Event-ID, a non-200 or non-event-stream answer
// fails it for good.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    PlainEvent = __hostEvent,
    Message = __hostMessageEvent,
    LocationURL = globalThis.URL,
    Controller = AbortController,
    Decoder = TextDecoder,
    StringValue = String;
  const hostTimeout = globalThis.setTimeout,
    hostClearTimeout = globalThis.clearTimeout,
    abort = AbortController.prototype.abort,
    signalOf = Object.getOwnPropertyDescriptor(AbortController.prototype, "signal").get,
    decode = TextDecoder.prototype.decode,
    getReader = ReadableStream.prototype.getReader;
  // readerPrototype must come from an in-realm probe — constructing
  // `new ReadableStream()` enters deno_web internals whose PromisePrototypeThen
  // hits V8's SpeciesConstructor against the extension-bound Promise. A page
  // that poisons `Promise[Symbol.species]` before an iframe is appended throws
  // during each child-frame EventSource bootstrap. We defend twice:
  //   1. Probe inside try/catch so a poisoned Promise doesn't halt bootstrap.
  //   2. If the probe throws, skip EventSource installation entirely — don't
  //      install a broken stub. Page code that feature-detects
  //      `'EventSource' in window` sees absence and handles it, whereas a
  //      cross-realm cache would let a page plant a hostile object on the top
  //      realm's `__esReaderPrototype` (plain enumerable global) and have the
  //      child-frame bootstrap adopt `read`/`cancel` from it — host-side code
  //      execution. (surf#245, turbo-dom#449 confirmed surf-side in cycle-2.)
  //
  // Important: we never read from a cross-realm cache slot. Each realm pays
  // the one-time probe cost, which is cheap on a fresh (un-poisoned) Promise.
  let readerPrototype = null;
  try {
    readerPrototype = Object.getPrototypeOf(apply(getReader, new ReadableStream(), []));
  } catch (_species) {
    /* fall through — EventSource won't be installed below */
  }
  if (!readerPrototype) {
    // Bail: don't install any EventSource in this realm. Feature detection
    // (`typeof EventSource === 'undefined'`) sees absence.
    return;
  }
  const read = readerPrototype.read,
    cancel = readerPrototype.cancel,
    headerGet = Headers.prototype.get,
    statusOf = Object.getOwnPropertyDescriptor(Response.prototype, "status").get,
    bodyOf = Object.getOwnPropertyDescriptor(Response.prototype, "body").get,
    urlOf = Object.getOwnPropertyDescriptor(Response.prototype, "url").get,
    headersOf = Object.getOwnPropertyDescriptor(Response.prototype, "headers").get;
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
  const DEFAULT_RETRY = 3000;

  const EventSource = __defineInterface(
    "EventSource",
    1,
    EventTarget,
    function (url, init = undefined) {
      const context = "Failed to construct 'EventSource'";
      if (!new.target)
        throw new ErrorType(
          `${context}: Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
        );
      if (!arguments.length)
        throw new ErrorType(`${context}: 1 argument required, but only 0 present.`);
      url = StringValue(url);
      let parsed;
      try {
        parsed = new LocationURL(url, apply(__baseURI, __document, []));
      } catch {
        throw new Exception(
          `${context}: Cannot open an EventSource to '${url}'. The URL is invalid.`,
          "SyntaxError",
        );
      }
      const target = new EventTarget();
      Object.setPrototypeOf(target, new.target.prototype);
      const record = {
        __proto__: null,
        url: parsed.href,
        withCredentials: !!init?.withCredentials,
        readyState: 0,
        lastEventId: "",
        retry: DEFAULT_RETRY,
        controller: null,
        reader: null,
        timer: 0,
      };
      apply(stateSet, states, [target, record]);
      ops.op_net_transport_hold(true);
      connect(target, record);
      return target;
    },
  );

  const dispatch = (source, event) => __dispatchTrusted(source, event);
  const shut = (record) => {
    if (record.readyState === 2) return false;
    record.readyState = 2;
    ops.op_net_transport_hold(false);
    return true;
  };
  // "Fail the connection": CLOSED for good, one error event.
  const fail = (source, record) => {
    if (shut(record)) dispatch(source, new PlainEvent("error"));
  };
  // "Reestablish the connection": CONNECTING, error, then retry after the reconnection time.
  const reestablish = (source, record) => {
    if (record.readyState === 2) return;
    record.readyState = 0;
    dispatch(source, new PlainEvent("error"));
    if (record.readyState !== 0) return;
    record.timer = apply(hostTimeout, globalThis, [
      () => record.readyState === 0 && connect(source, record),
      record.retry,
    ]);
  };
  const request = (record) => {
    const headers = { __proto__: null, Accept: "text/event-stream" };
    if (record.lastEventId) headers["Last-Event-ID"] = record.lastEventId;
    record.controller = new Controller();
    return {
      method: "GET",
      headers,
      cache: "no-store",
      mode: "cors",
      credentials: record.withCredentials ? "include" : "same-origin",
      destination: "",
      signal: apply(signalOf, record.controller, []),
      unref: true,
    };
  };
  const eventStream = (response) => {
    const type = apply(headerGet, apply(headersOf, response, []), ["content-type"]) ?? "";
    return type.split(";")[0].trim().toLowerCase() === "text/event-stream";
  };
  const connect = async (source, record) => {
    // connect-src is checked here (naming the full URL like Chrome) so the fetch path's
    // own check only ever sees allowed URLs.
    if (!ops.op_connect_allowed(__document, record.url)) {
      // The violation event is queued; the failure follows it as a task, as in Chrome.
      apply(hostTimeout, globalThis, [() => fail(source, record), 0]);
      return;
    }
    let response;
    try {
      response = await __fetchFromSource(
        record.url,
        request(record),
        ops.op_document_request_url(),
      );
    } catch (error) {
      // A policy block (or any failure before the request left) is final; a network error retries.
      const failure = __resourceFailure(error);
      if (!failure || failure.policyBlocked) fail(source, record);
      else reestablish(source, record);
      return;
    }
    if (record.readyState === 2) return;
    if (apply(statusOf, response, []) !== 200 || !eventStream(response))
      return fail(source, record);
    record.origin = new LocationURL(apply(urlOf, response, [])).origin;
    record.readyState = 1;
    dispatch(source, new PlainEvent("open"));
    await stream(source, record, apply(bodyOf, response, []));
  };
  const stream = async (source, record, body) => {
    const parser = { data: "", type: "", id: record.lastEventId, rest: "", lastCR: false };
    const decoder = new Decoder();
    record.reader = apply(getReader, body, []);
    try {
      for (;;) {
        const part = await apply(read, record.reader, []);
        if (record.readyState === 2) return;
        if (part.done) break;
        feed(source, record, parser, apply(decode, decoder, [part.value, { stream: true }]));
      }
    } catch {
      if (record.readyState === 2) return;
    }
    reestablish(source, record);
  };
  // Splits decoded text into lines (CRLF, LF or CR), carrying a partial line and a
  // trailing CR (whose LF may open the next chunk) across chunks.
  const feed = (source, record, parser, text) => {
    let at = 0;
    if (parser.lastCR && text[0] === "\n") at = 1;
    parser.lastCR = false;
    for (let index = at; index < text.length; index++) {
      const char = text[index];
      if (char !== "\n" && char !== "\r") continue;
      line(source, record, parser, parser.rest + text.slice(at, index));
      parser.rest = "";
      if (char === "\r") {
        if (index + 1 === text.length) parser.lastCR = true;
        else if (text[index + 1] === "\n") index++;
      }
      at = index + 1;
    }
    parser.rest += text.slice(at);
  };
  const line = (source, record, parser, text) => {
    if (text === "") return dispatchEvent(source, record, parser);
    if (text[0] === ":") return;
    const colon = text.indexOf(":");
    const name = colon < 0 ? text : text.slice(0, colon);
    let value = colon < 0 ? "" : text.slice(colon + 1);
    if (value[0] === " ") value = value.slice(1);
    field(record, parser, name, value);
  };
  const field = (record, parser, name, value) => {
    if (name === "event") parser.type = value;
    else if (name === "data") parser.data += value + "\n";
    else if (name === "id" && !value.includes("\0")) parser.id = value;
    else if (name === "retry" && /^[0-9]+$/.test(value)) record.retry = +value;
  };
  const dispatchEvent = (source, record, parser) => {
    record.lastEventId = parser.id;
    const data = parser.data,
      type = parser.type || "message";
    parser.data = parser.type = "";
    if (data === "" || record.readyState === 2) return;
    dispatch(
      source,
      new Message(type, {
        data: data.slice(0, -1),
        origin: record.origin,
        lastEventId: record.lastEventId,
      }),
    );
  };

  const P = EventSource.prototype;
  for (const [name, read] of [
    ["url", (record) => record.url],
    ["withCredentials", (record) => record.withCredentials],
    ["readyState", (record) => record.readyState],
  ])
    define(P, name, {
      __proto__: null,
      get: make(
        function () {
          return read(state(this));
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  for (const name of ["onopen", "onmessage", "onerror"]) __defineEventHandler(P, name, check);
  __defineConstants(EventSource, [
    ["CONNECTING", 0],
    ["OPEN", 1],
    ["CLOSED", 2],
  ]);
  define(P, "close", {
    __proto__: null,
    value: make(
      function () {
        const record = state(this);
        if (!shut(record)) return;
        apply(hostClearTimeout, globalThis, [record.timer]);
        if (record.reader) apply(cancel, record.reader, []).catch(() => {});
        if (record.controller) apply(abort, record.controller, []);
      },
      "close",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
