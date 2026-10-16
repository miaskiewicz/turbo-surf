(async () => {
  const out = {};
  const url = new URL("/echo", location.href).href;
  const stream = () =>
    new ReadableStream({
      start(c) {
        c.enqueue(new Uint8Array([65]));
        c.close();
      },
    });
  const check = async (name, run) => {
    try {
      out[name] = await run();
    } catch (error) {
      out[name] = { error: error.name };
    }
  };
  for (const kind of ["arrayBuffer", "getReader"]) {
    await check("response" + kind, async () => {
      let called = 0;
      const fake = {
        type: "application/fake",
        toString() {
          return "plain";
        },
        [kind]() {
          called++;
          return kind === "arrayBuffer"
            ? Promise.resolve(new Uint8Array([66]).buffer)
            : stream().getReader();
        },
      };
      const response = new Response(fake);
      return { text: await response.text(), type: response.headers.get("content-type"), called };
    });
  }
  await check("locked", async () => {
    const value = stream();
    value.getReader();
    new Request(url, { method: "POST", body: value, duplex: "half" });
    return "accepted";
  });
  await check("disturbed", async () => {
    const value = stream(),
      reader = value.getReader();
    await reader.read();
    reader.releaseLock();
    new Request(url, { method: "POST", body: value, duplex: "half" });
    return "accepted";
  });
  await check("nullOverride", async () => {
    const source = new Request(url, { method: "POST", body: "copied" });
    const target = new Request(source, { body: null });
    return { used: source.bodyUsed, locked: source.body.locked, text: await target.text() };
  });
  await check("usedOverride", async () => {
    const source = new Request(url, { method: "POST", body: "old" });
    await source.text();
    return new Request(source, { body: "new" }).text();
  });
  await check("cloneAfterBody", async () => {
    const source = new Request(url, { method: "POST", body: "clone" });
    void source.body;
    const target = source.clone();
    const response = await fetch(target);
    return {
      status: response.status,
      echo: await response.text(),
      used: target.bodyUsed,
      original: await source.text(),
    };
  });
  await check("responseSnapshot", async () => {
    const bytes = new Uint8Array([65]);
    const value = new Response(bytes);
    bytes[0] = 66;
    return value.text();
  });
  await check("blobPrivate", async () => {
    let called = 0;
    const value = new Blob(["blob"], { type: "text/plain" });
    Object.defineProperty(value, "type", {
      get() {
        called++;
        return "application/fake";
      },
    });
    value.arrayBuffer = () => {
      called++;
      return Promise.resolve(new Uint8Array([66]).buffer);
    };
    const request = new Request(url, { method: "POST", body: value });
    return { text: await request.text(), type: request.headers.get("content-type"), called };
  });
  await check("paramsPrivate", async () => {
    let called = 0;
    const value = new URLSearchParams("a=one");
    value.toString = () => {
      called++;
      return "fake";
    };
    const request = new Request(url, { method: "POST", body: value });
    return { text: await request.text(), called };
  });
  await check("streamPrivate", async () => {
    let called = 0;
    const value = stream();
    value.getReader = () => {
      called++;
      throw new Error("poison");
    };
    const request = new Request(url, { method: "POST", body: value, duplex: "half" });
    return { text: await request.text(), called };
  });
  await check("symbol", () => new Request(url, { method: "POST", body: Symbol("body") }));
  await check("nullObject", async () => {
    const value = {
      arrayBuffer: null,
      getReader: null,
      toString() {
        return "text";
      },
    };
    return new Request(url, { method: "POST", body: value }).text();
  });
  await check("byteViewOffset", async () => {
    const bytes = new Uint8Array([0, 65, 0]);
    return new Request(url, { method: "POST", body: new DataView(bytes.buffer, 1, 1) }).text();
  });
  await check("emptyStream", async () => {
    const value = new ReadableStream({
      start(c) {
        c.close();
      },
    });
    return new Request(url, { method: "POST", body: value, duplex: "half" }).text();
  });
  await check("invalidChunks", async () => {
    const results = [];
    for (const chunk of ["text", new ArrayBuffer(1), [65], new Uint8ClampedArray([65])]) {
      const value = new ReadableStream({
        start(c) {
          c.enqueue(chunk);
          c.close();
        },
      });
      try {
        results.push(await new Response(value).text());
      } catch (error) {
        results.push(error.name);
      }
    }
    return results;
  });
  return out;
})();
