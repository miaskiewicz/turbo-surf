(async () => {
  "use strict";
  // FileReader, XMLHttpRequest (+ XMLHttpRequestEventTarget/Upload) and WebSocket: descriptor
  // shapes, state machines, event order and argument handling. XHR talks to the probe page's own
  // origin: /xhr/json, /xhr/text and /xhr/missing are served by the capture harness.
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name];
    }
  };
  const fn = (f) => (typeof f === "function" ? { name: f.name, length: f.length, native: Function.prototype.toString.call(f) === `function ${f.name}() { [native code] }` } : typeof f);
  const desc = (o, k) => {
    const d = Object.getOwnPropertyDescriptor(o, k);
    if (!d) return null;
    const r = { c: d.configurable, e: d.enumerable };
    if ("value" in d) {
      r.w = d.writable;
      r.v = typeof d.value === "function" ? fn(d.value) : d.value !== null && typeof d.value === "object" ? "object" : d.value;
    } else {
      r.get = fn(d.get);
      r.set = fn(d.set);
    }
    return r;
  };
  const shape = (o) => Object.fromEntries(Reflect.ownKeys(o).map((k) => [String(k), k]).sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(([s, k]) => [s, desc(o, k)]));
  const tick = (ms = 20) => new Promise((done) => setTimeout(done, ms));
  const out = { interfaces: {} };
  for (const name of ["FileReader", "XMLHttpRequestEventTarget", "XMLHttpRequestUpload", "XMLHttpRequest", "WebSocket"]) {
    const C = globalThis[name];
    out.interfaces[name] = { ctor: shape(C), proto: shape(C.prototype), parent: Object.getPrototypeOf(C).name || "Function", global: desc(globalThis, name), construct: run(() => new C("ws://127.0.0.1:1/"))[0] };
  }
  const events = (target, names, log) => {
    for (const type of names)
      target.addEventListener(type, (event) => {
        const row = [type, event.isTrusted, Object.prototype.toString.call(event), event.target === target];
        if ("loaded" in event) row.push(event.lengthComputable, event.loaded, event.total);
        if ("code" in event) row.push(event.wasClean, event.code, event.reason);
        if ("readyState" in target) row.push(target.readyState);
        log.push(row);
      });
  };
  // ---- FileReader ----
  const fr = {};
  const finished = (reader) => new Promise((done) => reader.addEventListener("loadend", () => setTimeout(done, 0)));
  const read = async (method, blob, ...args) => {
    const reader = new FileReader();
    const log = [];
    events(reader, ["loadstart", "progress", "load", "abort", "error", "loadend"], log);
    const initial = [reader.readyState, reader.result, reader.error];
    reader[method](blob, ...args);
    const sync = [reader.readyState, reader.result, log.length];
    const again = run(() => reader.readAsText(blob))[1];
    await finished(reader);
    let result = reader.result;
    if (result instanceof ArrayBuffer) result = ["ArrayBuffer", [...new Uint8Array(result)]];
    return { initial, sync, again, log, readyState: reader.readyState, result, error: reader.error };
  };
  const bytes = new Blob([new Uint8Array([0, 1, 254, 255, 104, 105])], { type: "application/x-probe" });
  fr.arrayBuffer = await read("readAsArrayBuffer", bytes);
  fr.binary = await read("readAsBinaryString", bytes);
  fr.dataURL = await read("readAsDataURL", bytes);
  fr.dataURLUntyped = await read("readAsDataURL", new Blob(["hi"]));
  fr.text = await read("readAsText", new Blob(["héllo \u{1F600}"]));
  fr.textLatin = await read("readAsText", new Blob([new Uint8Array([104, 233])]), "iso-8859-1");
  fr.textBom = await read("readAsText", new Blob([new Uint8Array([0xfe, 0xff, 0, 104])]));
  fr.textBadLabel = await read("readAsText", new Blob(["x"]), "bogus-label");
  fr.empty = await read("readAsText", new Blob([]));
  {
    const reader = new FileReader();
    const log = [];
    events(reader, ["loadstart", "progress", "load", "abort", "error", "loadend"], log);
    reader.readAsText(new Blob(["abort me"]));
    reader.abort();
    const afterAbort = [reader.readyState, reader.result, log.length];
    await tick(50);
    fr.abort = { afterAbort, log, readyState: reader.readyState, result: reader.result };
    const idle = new FileReader();
    idle.abort();
    fr.idleAbort = [idle.readyState, idle.result];
  }
  fr.errors = [run(() => new FileReader().readAsText())[1], run(() => new FileReader().readAsText({}))[1], run(() => new FileReader().readAsDataURL("x"))[1], run(() => FileReader())[1]];
  {
    const reader = new FileReader();
    const order = [];
    reader.onload = () => order.push("onload");
    reader.addEventListener("load", () => order.push("listener"));
    reader.onloadend = (event) => order.push(["onloadend", event.type]);
    reader.readAsText(new Blob(["z"]));
    await finished(reader);
    fr.handlers = [order, typeof reader.onload, (reader.onprogress = 5, reader.onprogress)];
  }
  out.fileReader = fr;
  // ---- XMLHttpRequest ----
  const xhr = {};
  const request = async (setup) => {
    const x = new XMLHttpRequest();
    const log = [];
    events(x, ["readystatechange", "loadstart", "progress", "load", "error", "abort", "timeout", "loadend"], log);
    const done = new Promise((resolve) => x.addEventListener("loadend", () => setTimeout(resolve, 0)));
    const before = setup(x);
    await done;
    return {
      before,
      log,
      readyState: x.readyState,
      status: x.status,
      statusText: x.statusText,
      responseURL: x.responseURL.replace(location.origin, ""),
      headers: x.getAllResponseHeaders().split("\r\n").filter((line) => /^(content-type|x-probe|set-cookie)/i.test(line)),
      header: [x.getResponseHeader("X-PROBE"), x.getResponseHeader("set-cookie"), x.getResponseHeader("absent")],
      responseType: x.responseType,
      response: x.response instanceof ArrayBuffer ? ["ArrayBuffer", x.response.byteLength] : x.response instanceof Blob ? ["Blob", x.response.size, x.response.type] : x.response,
      responseText: run(() => x.responseText),
      responseXML: run(() => x.responseXML),
    };
  };
  xhr.initial = (() => {
    const x = new XMLHttpRequest();
    return [x.readyState, x.status, x.statusText, x.response, x.responseText, x.responseURL, x.responseType, x.timeout, x.withCredentials, Object.prototype.toString.call(x.upload), x.upload === x.upload, x.responseXML, x.getAllResponseHeaders(), x.getResponseHeader("a")];
  })();
  xhr.json = await request((x) => {
    x.open("get", "/xhr/json");
    const opened = [x.readyState];
    x.setRequestHeader("X-Custom", "a");
    x.setRequestHeader("x-custom", "b");
    x.responseType = "json";
    x.send();
    opened.push(x.readyState, run(() => x.send())[1], run(() => (x.responseType = "text"))[0]);
    return opened;
  });
  xhr.text = await request((x) => {
    x.open("GET", "/xhr/text");
    x.send(null);
  });
  xhr.arraybuffer = await request((x) => {
    x.open("GET", "/xhr/text");
    x.responseType = "arraybuffer";
    x.send();
  });
  xhr.blob = await request((x) => {
    x.open("GET", "/xhr/json");
    x.responseType = "blob";
    x.send();
  });
  xhr.missing = await request((x) => {
    x.open("GET", "/xhr/missing");
    x.send();
  });
  xhr.override = await request((x) => {
    x.open("GET", "/xhr/text");
    x.overrideMimeType("text/plain; charset=iso-8859-1");
    x.send();
  });
  {
    const x = new XMLHttpRequest();
    const log = [];
    events(x, ["readystatechange", "loadstart", "progress", "load", "error", "abort", "loadend"], log);
    x.open("GET", "/xhr/json");
    x.send();
    x.abort();
    const after = [x.readyState, x.status, x.response];
    await tick(100);
    xhr.abort = { log, after, readyState: x.readyState };
  }
  xhr.errors = [
    run(() => new XMLHttpRequest().send())[1],
    run(() => new XMLHttpRequest().setRequestHeader("a", "b"))[1],
    run(() => new XMLHttpRequest().open("TRACE", "/"))[1],
    run(() => new XMLHttpRequest().open("bad method", "/"))[1],
    run(() => new XMLHttpRequest().open("GET", "http://[bad"))[1],
    run(() => new XMLHttpRequest().open("GET"))[1],
    run(() => {
      const x = new XMLHttpRequest();
      x.open("GET", "/");
      x.setRequestHeader("bad name", "v");
    })[1],
    run(() => {
      const x = new XMLHttpRequest();
      x.responseType = "bogus";
      return x.responseType;
    })[1],
    run(() => {
      const x = new XMLHttpRequest();
      x.responseType = "json";
      return x.responseText;
    })[1],
    run(() => XMLHttpRequest())[1],
    run(() => Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, "status").get.call({}))[1],
    run(() => new XMLHttpRequestUpload())[1],
    run(() => new XMLHttpRequestEventTarget())[1],
  ];
  {
    const x = new XMLHttpRequest();
    const methods = [];
    for (const method of ["get", "Post", "delete", "patch", "options", "head", "PUT", "custom"]) {
      x.open(method, "/");
      methods.push(method);
    }
    xhr.methods = methods.length;
    const order = [];
    x.onreadystatechange = () => order.push("on" + x.readyState);
    x.open("GET", "/xhr/text");
    xhr.reopen = order;
  }
  out.xhr = xhr;
  // ---- WebSocket ----
  const ws = {};
  ws.construct = [
    run(() => new WebSocket())[1],
    run(() => new WebSocket("ftp://127.0.0.1/"))[1],
    run(() => new WebSocket("ws://127.0.0.1:1/#frag"))[1],
    run(() => new WebSocket("ws://127.0.0.1:1/", ["a", "a"]))[1],
    run(() => new WebSocket("ws://127.0.0.1:1/", "bad protocol"))[1],
    run(() => WebSocket("ws://127.0.0.1:1/"))[1],
  ];
  {
    const socket = new WebSocket("http://127.0.0.1:1/path?q", ["chat", "superchat"]);
    const log = [];
    events(socket, ["open", "message", "error", "close"], log);
    ws.initial = [socket.url, socket.readyState, socket.protocol, socket.extensions, socket.bufferedAmount, socket.binaryType];
    socket.binaryType = "arraybuffer";
    const binary = socket.binaryType;
    socket.binaryType = "bogus";
    ws.binaryType = [binary, socket.binaryType];
    ws.sendConnecting = run(() => socket.send("x"))[1];
    ws.closeErrors = [run(() => socket.close(1001))[1], run(() => socket.close(2999))[1], run(() => socket.close(1000, "x".repeat(124)))[1]];
    socket.close(1000, "bye");
    ws.afterClose = [socket.readyState, log.length];
    await tick(100);
    ws.log = log;
    ws.final = [socket.readyState, run(() => socket.send("late"))[0], socket.bufferedAmount];
  }
  {
    const socket = new WebSocket("wss://127.0.0.1:1/");
    ws.secure = socket.url;
    socket.close();
  }
  out.webSocket = ws;
  return out;
})()
