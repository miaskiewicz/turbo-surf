/* eslint-disable unicorn/consistent-function-scoping, no-await-in-loop -- The oracle is a self-contained expression evaluated in the page realm. Sequential cancel acknowledgement is the behavior being measured. */
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
  for (const order of ["transformFirst", "readableFirst", "writableFirst"]) {
    const source = new TransformStream(),
      buffer = new ArrayBuffer(8);
    const list =
      order === "transformFirst"
        ? [buffer, source, source.readable]
        : order === "readableFirst"
          ? [buffer, source.readable, source]
          : [buffer, source.writable, source];
    const error = errorName(() => structuredClone({ source }, { transfer: list }));
    out[order] = {
      error,
      bufferBytes: buffer.byteLength,
      readableLocked: source.readable.locked,
      writableLocked: source.writable.locked,
    };
  }
  {
    const source = new ReadableStream({
      type: "bytes",
      start(controller) {
        controller.enqueue(new Uint8Array([1, 2]));
        controller.close();
      },
    });
    const received = structuredClone(source, { transfer: [source] });
    out.receivedByteStreamBYOB = errorName(() => received.getReader({ mode: "byob" }));
    out.receivedByteStreamRead = Array.from((await received.getReader().read()).value);
  }
  for (const kind of ["readable", "writable"]) {
    let accepted;
    const source =
      kind === "readable"
        ? new ReadableStream({
            cancel(value) {
              accepted = value;
            },
          })
        : new WritableStream({
            abort(value) {
              accepted = value;
            },
          });
    const target = structuredClone(source, { transfer: [source] });
    let error = null;
    try {
      if (kind === "readable") await target.cancel(() => {});
      else await target.abort(() => {});
    } catch (value) {
      error = value.name;
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
    out[kind + "UncloneableReason"] = {
      error,
      sourceReceived:
        accepted === undefined ? null : { name: accepted.name, message: accepted.message },
    };
  }
  {
    const source = new ReadableStream({
        start(controller) {
          controller.enqueue(() => {});
          controller.close();
        },
      }),
      target = structuredClone(source, { transfer: [source] });
    try {
      await target.getReader().read();
      out.uncloneableChunk = null;
    } catch (error) {
      out.uncloneableChunk = { name: error.name };
    }
  }
  {
    const source = new ReadableStream({
      start(controller) {
        controller.enqueue(5);
        controller.close();
      },
    });
    const first = structuredClone(source, { transfer: [source] });
    const second = structuredClone(first, { transfer: [first] });
    out.retransfer = {
      sourceLocked: source.locked,
      intermediateLocked: first.locked,
      value: (await second.getReader().read()).value,
    };
  }
  return out;
})();
