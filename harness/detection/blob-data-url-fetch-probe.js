(async () => {
  // fetch() and XMLHttpRequest over blob: and data: URLs: the blob URL shape,
  // revocation, status/headers/url, Range on blob URLs, method checks, the
  // same-origin rule for blob URLs, and data: decoding for text and documents.
  const out = {};
  const origin = location.origin;
  const shape = (url) =>
    typeof url === "string" &&
    url.startsWith(`blob:${origin}/`) &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      url.slice(`blob:${origin}/`.length),
    );
  const error = (value) => [value?.name ?? typeof value, value?.message ?? String(value)];
  const response = async (url, init) => {
    try {
      const value = await fetch(url, init);
      return [
        value.status,
        value.statusText,
        value.ok,
        value.type,
        value.url === url,
        value.redirected,
        [...value.headers],
        await value.text(),
      ];
    } catch (value) {
      return ["error", ...error(value)];
    }
  };
  const xhr = (url, type = "", method = "GET", headers = {}) =>
    new Promise((resolve) => {
      const request = new XMLHttpRequest();
      const events = [];
      for (const name of ["load", "error", "abort", "loadend"])
        request.addEventListener(name, () => events.push(name));
      request.addEventListener("loadend", () =>
        resolve([
          events,
          request.status,
          request.statusText,
          request.responseURL === url,
          request.getAllResponseHeaders(),
          type === "document"
            ? [
                request.responseXML?.documentElement?.nodeName ?? null,
                request.responseXML?.contentType ?? null,
              ]
            : request.responseText,
        ]),
      );
      try {
        request.open(method, url);
        request.responseType = type;
        for (const [name, value] of Object.entries(headers)) request.setRequestHeader(name, value);
        request.send();
      } catch (value) {
        resolve(["throw", ...error(value)]);
      }
    });

  const text = new Blob(["hello blob"], { type: "text/plain" });
  const url = URL.createObjectURL(text);
  out.shape = [shape(url), URL.createObjectURL(text) !== url];
  out.fetch = await response(url);
  out.fetchTwice = await response(url);
  const untyped = URL.createObjectURL(new Blob([new Uint8Array([1, 2, 3])]));
  out.untyped = await response(untyped);
  const upper = URL.createObjectURL(new Blob(["x"], { type: "Text/HTML;Charset=UTF-8" }));
  out.typed = await response(upper);
  out.empty = await response(URL.createObjectURL(new Blob([])));
  out.range = await response(url, { headers: { Range: "bytes=1-3" } });
  out.rangeOpen = await response(url, { headers: { Range: "bytes=6-" } });
  out.rangeSuffix = await response(url, { headers: { Range: "bytes=-4" } });
  out.rangeBad = await response(url, { headers: { Range: "bytes=20-30" } });
  out.rangeMulti = await response(url, { headers: { Range: "bytes=0-1,3-4" } });
  out.post = await response(url, { method: "POST", body: "x" });
  out.head = await response(url, { method: "HEAD" });
  out.fragment = await response(`${url}#frag`);
  out.query = await response(`${url}?q=1`);
  out.missing = await response(`blob:${origin}/00000000-0000-4000-8000-000000000000`);
  out.foreign = await response(url.replace(origin, "http://example.invalid"));
  out.xhr = await xhr(url);
  out.xhrRange = await xhr(url, "", "GET", { Range: "bytes=0-4" });
  out.xhrPost = await xhr(url, "", "POST");
  out.xhrArray = await new Promise((resolve) => {
    const request = new XMLHttpRequest();
    request.open("GET", untyped);
    request.responseType = "arraybuffer";
    request.onloadend = () =>
      resolve([request.status, Array.from(new Uint8Array(request.response))]);
    request.send();
  });
  const xml = URL.createObjectURL(new Blob(["<root><child/></root>"], { type: "application/xml" }));
  out.xhrDocument = await xhr(xml, "document");
  out.syncXhr = (() => {
    const request = new XMLHttpRequest();
    request.open("GET", url, false);
    request.send();
    return [request.status, request.responseText, request.getResponseHeader("content-length")];
  })();
  URL.revokeObjectURL(url);
  out.revoked = await response(url);
  out.revokedXhr = await xhr(url);
  out.revokeTwice = (() => {
    try {
      URL.revokeObjectURL(url);
      URL.revokeObjectURL("not a url");
      URL.revokeObjectURL(`blob:${origin}/nope`);
      return "ok";
    } catch (value) {
      return error(value);
    }
  })();
  out.createErrors = [undefined, "x", {}].map((value) => {
    try {
      return URL.createObjectURL(value);
    } catch (value2) {
      return error(value2);
    }
  });
  out.file = await response(
    URL.createObjectURL(new File(["file body"], "a.txt", { type: "text/csv" })),
  );

  // Same-origin frames share the store; an opaque-origin frame mints blob:null.
  const frame = document.createElement("iframe");
  frame.srcdoc = "<!doctype html><body>";
  document.body.append(frame);
  await new Promise((resolve) => (frame.onload = resolve));
  const childUrl = frame.contentWindow.URL.createObjectURL(
    new frame.contentWindow.Blob(["from child"]),
  );
  out.childShape = shape(childUrl);
  out.childInParent = await response(childUrl);
  out.parentInChild = await new Promise((resolve) => {
    const shared = URL.createObjectURL(new Blob(["from parent"]));
    frame.contentWindow
      .fetch(shared)
      .then((value) => value.text())
      .then(resolve, (value) => resolve(error(value)));
  });
  const sandboxed = document.createElement("iframe");
  sandboxed.sandbox = "allow-scripts";
  sandboxed.srcdoc =
    "<!doctype html><script>parent.postMessage(URL.createObjectURL(new Blob(['opaque'])), '*')</script>";
  const minted = new Promise((resolve) =>
    addEventListener("message", (event) => resolve(event.data), { once: true }),
  );
  document.body.append(sandboxed);
  const opaque = await minted;
  out.opaqueShape = [
    typeof opaque === "string" && opaque.startsWith("blob:null/"),
    /^[0-9a-f-]{36}$/.test(String(opaque).slice("blob:null/".length)),
  ];
  out.opaqueInParent = await response(opaque);
  frame.remove();
  sandboxed.remove();

  // data: URLs
  out.data = await response("data:text/plain,hello%20data");
  out.dataDefault = await response("data:,abc");
  out.dataBase64 = await response("data:application/octet-stream;base64,aGk=");
  out.dataCharset = await response("data:text/html;charset=utf-8,%3Cb%3E");
  out.dataBadBase64 = await response("data:text/plain;base64,%%%");
  out.dataPost = await response("data:text/plain,posted", { method: "POST", body: "x" });
  out.dataRange = await response("data:text/plain,abcdef", { headers: { Range: "bytes=1-2" } });
  out.dataXhr = await xhr("data:text/plain,xhr%20text");
  out.dataXhrDocument = await xhr(
    "data:application/xml,%3Croot%3E%3Cleaf%2F%3E%3C%2Froot%3E",
    "document",
  );
  out.dataXhrHtml = await xhr("data:text/html,%3Cp%3Ehi", "document");
  out.dataXhrPost = await xhr("data:text/plain,abc", "", "POST");
  return JSON.stringify(out);
})();
