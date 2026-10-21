(async () => {
  const controller = new AbortController(),
    signal = controller.signal,
    dependent = AbortSignal.any([signal]);
  const events = [],
    target = new EventTarget();
  let hits = 0;
  target.addEventListener("probe", () => hits++, { signal: dependent });
  dependent.addEventListener("abort", (event) =>
    events.push([
      event.isTrusted,
      event.target === dependent,
      event.currentTarget === dependent,
      event instanceof Event,
    ]),
  );
  dependent.onabort = () => events.push("handler");
  target.dispatchEvent(new Event("probe"));
  const reason = { authored: "original" };
  controller.abort(reason);
  target.dispatchEvent(new Event("probe"));
  let abortSame = false;
  try {
    dependent.throwIfAborted();
  } catch (error) {
    abortSame = error === reason;
  }
  const queued = [],
    stream = new ReadableStream({
      start(c) {
        queued.push(c);
        c.enqueue("first");
      },
      pull(c) {
        c.enqueue("second");
        c.close();
      },
    });
  const reader = stream.getReader(),
    first = await reader.read(),
    second = await reader.read(),
    end = await reader.read();
  await reader.closed;
  reader.releaseLock();
  const chunks = [],
    write = new WritableStream({
      write(value, c) {
        chunks.push([
          value,
          c instanceof WritableStreamDefaultController,
          c.signal instanceof AbortSignal,
        ]);
      },
    });
  await new ReadableStream({
    start(c) {
      c.enqueue("a");
      c.enqueue("b");
      c.close();
    },
  }).pipeTo(write);
  const bytes = new ReadableStream({
      type: "bytes",
      start(c) {
        c.enqueue(new Uint8Array([0, 255, 128]));
        c.close();
      },
    }),
    byob = bytes.getReader({ mode: "byob" });
  const view = new Uint8Array(8),
    read = await byob.read(view);
  byob.releaseLock();
  const transform = new TransformStream({
    transform(value, c) {
      c.enqueue(value.toUpperCase());
    },
  });
  const transformed = new ReadableStream({
    start(c) {
      c.enqueue("hi");
      c.close();
    },
  })
    .pipeThrough(transform)
    .getReader();
  const transformedValue = await transformed.read(),
    transformedEnd = await transformed.read();
  transformed.releaseLock();
  let forged = false;
  try {
    Object.getOwnPropertyDescriptor(AbortSignal.prototype, "aborted").get.call({});
  } catch (error) {
    forged = error instanceof TypeError;
  }
  const own = (value) =>
    Reflect.ownKeys(value).map((value) => (typeof value === "symbol" ? String(value) : value));
  const iteratorStream = new ReadableStream({
      start(c) {
        c.enqueue("iterator");
        c.close();
      },
    }),
    iterator = iteratorStream.values(),
    iteratorProto = Object.getPrototypeOf(iterator);
  const iteratorShape = {
    own: own(iterator),
    proto: own(iteratorProto),
    tag: Object.prototype.toString.call(iterator),
    result: await iterator.next(),
  };
  await iterator.return();
  const strategy = new ByteLengthQueuingStrategy({ highWaterMark: 4 });
  const strategyShape = {
    own: own(strategy),
    sizeOwn: own(strategy.size),
    name: strategy.size.name,
    length: strategy.size.length,
    native: /\[native code\]/.test(String(strategy.size)),
    size: strategy.size(new Uint8Array(3)),
    same: strategy.size === new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size,
  };
  const timeoutShape = await new Promise((resolve) => {
    const timeout = AbortSignal.timeout(5);
    timeout.addEventListener("abort", (event) =>
      resolve({
        aborted: timeout.aborted,
        name: timeout.reason.name,
        brand: timeout.reason instanceof DOMException,
        target: event.target === timeout,
        trusted: event.isTrusted,
      }),
    );
  });
  const countStrategy = new CountQueuingStrategy({ highWaterMark: 2 });
  const countShape = {
    length: countStrategy.size.length,
    size: countStrategy.size(),
    native: /\[native code\]/.test(String(countStrategy.size)),
    same: countStrategy.size === new CountQueuingStrategy({ highWaterMark: 1 }).size,
  };
  const frame = document.createElement("iframe");
  document.body.append(frame);
  const child = frame.contentWindow;
  const authoredError = new TypeError("authored"),
    foreign = new child.ReadableStream({
      start(c) {
        c.error(authoredError);
      },
    }),
    foreignReader = foreign.getReader();
  const foreignPromise = ReadableStreamDefaultReader.prototype.read.call(foreignReader);
  let sameError = false;
  try {
    await foreignPromise;
  } catch (error) {
    sameError = error === authoredError;
  }
  foreignReader.closed.catch(() => {});
  foreignReader.releaseLock();
  const borrowed = child.ReadableStreamDefaultReader.prototype.read.call({});
  let localError = false;
  try {
    await borrowed;
  } catch (error) {
    localError = error instanceof child.TypeError;
  }
  const constructorErrors = [
    ReadableStream,
    WritableStream,
    TransformStream,
    ByteLengthQueuingStrategy,
    CountQueuingStrategy,
    AbortController,
  ].map((C) => {
    try {
      C();
      return false;
    } catch (error) {
      return error instanceof TypeError;
    }
  });
  const dictionaryErrors = [ReadableStream, WritableStream, TransformStream].map((C) => {
    try {
      new C(1);
      return false;
    } catch (error) {
      return error instanceof TypeError;
    }
  });
  class StreamSubclass extends ReadableStream {}
  class AbortSubclass extends AbortController {}
  const subclasses =
    new StreamSubclass() instanceof StreamSubclass && new AbortSubclass() instanceof AbortSubclass;
  let sourceError = false,
    strategyError = false;
  try {
    new child.ReadableStream({
      get type() {
        throw authoredError;
      },
    });
  } catch (error) {
    sourceError = error === authoredError;
  }
  try {
    new child.ReadableStream(
      {},
      {
        get highWaterMark() {
          throw authoredError;
        },
      },
    );
  } catch (error) {
    strategyError = error === authoredError;
  }
  const foreignShape = {
    promise: foreignPromise instanceof Promise && !(foreignPromise instanceof child.Promise),
    sameError,
    borrowedPromise: borrowed instanceof child.Promise,
    localError,
  };
  frame.remove();
  return {
    abort: {
      aborted: signal.aborted && dependent.aborted,
      reason: signal.reason === reason && dependent.reason === reason,
      abortSame,
      events,
      hits,
      forged,
      own: own(signal),
      controllerOwn: own(controller),
    },
    stream: {
      own: own(stream),
      controller: queued[0] instanceof ReadableStreamDefaultController,
      readerOwn: own(reader),
      first,
      second,
      end,
      locked: stream.locked,
      chunks,
      byob: Array.from(read.value),
      viewDetached: view.byteLength === 0,
      transformedValue,
      transformedEnd,
      iteratorShape,
      strategyShape,
      countShape,
      timeoutShape,
      foreignShape,
      constructorErrors,
      dictionaryErrors,
      subclasses,
      sourceError,
      strategyError,
    },
  };
})();
