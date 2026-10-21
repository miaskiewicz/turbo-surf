(async () => {
  const out = {};
  const errorName = (callback) => {
    try {
      callback();
      return null;
    } catch (error) {
      return error.name;
    }
  };
  for (const [name, create] of [
    ["readable", () => new ReadableStream()],
    ["writable", () => new WritableStream()],
    ["transform", () => new TransformStream()],
  ]) {
    const stream = create();
    out[name + "RequiresTransfer"] = errorName(() => structuredClone(stream));
    out[name + "Duplicate"] = errorName(() =>
      structuredClone(stream, { transfer: [stream, stream] }),
    );
  }
  {
    const source = new ReadableStream();
    const reader = source.getReader();
    out.lockedReadable = errorName(() => structuredClone(source, { transfer: [source] }));
    reader.releaseLock();
  }
  {
    const source = new WritableStream();
    const writer = source.getWriter();
    out.lockedWritable = errorName(() => structuredClone(source, { transfer: [source] }));
    writer.releaseLock();
  }
  {
    const chunk = { bytes: new Uint8Array([65, 66]) };
    const source = new ReadableStream({
      start(controller) {
        controller.enqueue(chunk);
        controller.close();
      },
    });
    const received = structuredClone(source, { transfer: [source] });
    const originalLocked = source.locked;
    const reader = received.getReader();
    const first = await reader.read();
    const last = await reader.read();
    out.readable = {
      originalLocked,
      receiverBrand: received instanceof ReadableStream,
      ownMembers: Object.getOwnPropertyNames(received),
      differentChunk: first.value !== chunk,
      bytes: Array.from(first.value.bytes),
      originalBytes: chunk.bytes.byteLength,
      done: last.done,
    };
  }
  {
    let accepted, written, closed;
    const didWrite = new Promise((resolve) => {
      written = resolve;
    });
    const didClose = new Promise((resolve) => {
      closed = resolve;
    });
    const source = new WritableStream({
      write(value) {
        accepted = value;
        written();
      },
      close() {
        closed();
      },
    });
    const received = structuredClone(source, { transfer: [source] });
    const originalLocked = source.locked;
    const chunk = { value: 7 };
    const writer = received.getWriter();
    await writer.write(chunk);
    await didWrite;
    await writer.close();
    await didClose;
    out.writable = {
      originalLocked,
      receiverBrand: received instanceof WritableStream,
      ownMembers: Object.getOwnPropertyNames(received),
      differentChunk: accepted !== chunk,
      accepted,
    };
  }
  {
    const source = new TransformStream({
      transform(value, controller) {
        controller.enqueue(value + 1);
      },
    });
    const received = structuredClone(source, { transfer: [source] });
    const originalLocked = [source.readable.locked, source.writable.locked];
    const reader = received.readable.getReader();
    const writer = received.writable.getWriter();
    const writing = writer.write(8);
    const first = await reader.read();
    await writing;
    await writer.close();
    const last = await reader.read();
    out.transform = {
      originalLocked,
      receiverBrand: received instanceof TransformStream,
      ownMembers: Object.getOwnPropertyNames(received),
      value: first.value,
      done: last.done,
    };
  }
  {
    const source = new ReadableStream();
    const buffer = new ArrayBuffer(8);
    const error = errorName(() =>
      structuredClone({ stream: source, method() {} }, { transfer: [buffer, source] }),
    );
    out.failedGraph = { error, bufferBytes: buffer.byteLength, locked: source.locked };
  }
  {
    const source = new ReadableStream();
    const buffer = new ArrayBuffer(8);
    let reader;
    const value = {
      get lockDuringSerialization() {
        reader = source.getReader();
        return 0;
      },
      stream: source,
    };
    const error = errorName(() => structuredClone(value, { transfer: [buffer, source] }));
    out.lateLock = { error, bufferBytes: buffer.byteLength, locked: source.locked };
    reader?.releaseLock();
  }
  {
    const source = new ReadableStream();
    const result = structuredClone(7, { transfer: [source] });
    out.unreachable = { result, locked: source.locked };
  }
  out.forgedReadable = errorName(() => {
    const forged = Object.create(ReadableStream.prototype);
    structuredClone(forged, { transfer: [forged] });
  });
  {
    const source = new ReadableStream({
      start(controller) {
        controller.enqueue(11);
        controller.close();
      },
    });
    const channel = new MessageChannel();
    const eventPromise = new Promise((resolve) => {
      channel.port2.onmessage = resolve;
    });
    channel.port1.postMessage({ stream: source }, [source]);
    const originalLocked = source.locked;
    const event = await eventPromise;
    const reader = event.data.stream.getReader();
    const first = await reader.read();
    out.messagePort = {
      originalLocked,
      receiverBrand: event.data.stream instanceof ReadableStream,
      ports: event.ports.length,
      value: first.value,
      trusted: event.isTrusted,
    };
    channel.port1.close();
    channel.port2.close();
  }
  {
    const source = new ReadableStream({
      start(controller) {
        controller.enqueue(12);
        controller.close();
      },
    });
    const eventPromise = new Promise((resolve) => {
      addEventListener("message", resolve, { once: true });
    });
    postMessage({ stream: source }, "*", [source]);
    const originalLocked = source.locked;
    const event = await eventPromise;
    const reader = event.data.stream.getReader();
    const first = await reader.read();
    out.window = {
      originalLocked,
      receiverBrand: event.data.stream instanceof ReadableStream,
      ports: event.ports.length,
      value: first.value,
      trusted: event.isTrusted,
    };
  }
  for (const kind of ["readable", "writable"]) {
    let receivedReason, acknowledge;
    const handled = new Promise((resolve) => {
      acknowledge = resolve;
    });
    const reason = { authored: kind };
    const accept = (value) => {
      receivedReason = value;
      acknowledge();
    };
    const source =
      kind === "readable"
        ? new ReadableStream({ cancel: accept })
        : new WritableStream({ abort: accept });
    const received = structuredClone(source, { transfer: [source] });
    if (kind === "readable") await received.cancel(reason);
    else await received.abort(reason);
    await handled;
    out[kind + "Reason"] = { differentReason: receivedReason !== reason, receivedReason };
  }
  {
    const source = new ReadableStream({
      start(controller) {
        controller.error(new TypeError("authored"));
      },
    });
    const received = structuredClone(source, { transfer: [source] });
    try {
      await received.getReader().read();
      out.sourceError = null;
    } catch (error) {
      out.sourceError = {
        name: error.name,
        message: error.message,
        realm: error instanceof TypeError,
      };
    }
  }
  return out;
})();
