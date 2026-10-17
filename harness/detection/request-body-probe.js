(async () => {
  const out = {};
  const url = new URL("/echo", location.href).href;
  const stream = () =>
    new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array([65]));
        controller.close();
      },
    });
  for (const [name, init] of [
    ["missingDuplex", { body: stream() }],
    ["invalidDuplex", { body: "text", duplex: "full" }],
    ["validDuplex", { body: stream(), duplex: "half" }],
    ["streamKeepalive", { body: stream(), duplex: "half", keepalive: true }],
    ["streamNoCors", { body: stream(), duplex: "half", mode: "no-cors" }],
  ]) {
    try {
      const request = new Request(url, { method: "POST", ...init });
      out[name] = { result: "accepted", duplex: request.duplex, used: request.bodyUsed };
    } catch (error) {
      out[name] = { error: error.name };
    }
  }
  const source = new Request(url, { method: "POST", body: "text" });
  const copied = new Request(source);
  out.constructFromRequest = {
    sourceUsed: source.bodyUsed,
    copiedUsed: copied.bodyUsed,
    text: await copied.text(),
  };
  const original = new Request(url, { method: "POST", body: "clone" });
  const cloned = original.clone();
  out.clone = {
    sourceUsed: original.bodyUsed,
    cloneUsed: cloned.bodyUsed,
    source: await original.text(),
    clone: await cloned.text(),
  };
  for (const kind of ["arrayBuffer", "getReader"]) {
    let called = 0;
    const fake = {
      toString() {
        return "plain-object";
      },
      [kind]() {
        called++;
        return kind === "arrayBuffer"
          ? Promise.resolve(new Uint8Array([66]).buffer)
          : stream().getReader();
      },
      type: "application/fake",
    };
    const request = new Request(url, { method: "POST", body: fake });
    out[kind + "Duck"] = {
      type: request.headers.get("content-type"),
      text: await request.text(),
      called,
    };
  }
  const bytes = new Uint8Array([65]);
  const binary = new Request(url, { method: "POST", body: bytes });
  bytes[0] = 66;
  out.bufferSnapshot = await binary.text();
  const params = new URLSearchParams("first=one");
  const form = new Request(url, { method: "POST", body: params });
  params.set("first", "two");
  out.paramsSnapshot = await form.text();
  const upload = new Request(url, { method: "POST", body: stream(), duplex: "half" });
  try {
    out.streamHttp1 = { status: (await fetch(upload)).status, used: upload.bodyUsed };
  } catch (error) {
    out.streamHttp1 = { error: error.name, used: upload.bodyUsed };
  }
  return out;
})();
