(async () => {
  const result = {},
    capture = (run) => {
      try {
        return run();
      } catch (error) {
        return { error: error.name, code: error.code };
      }
    },
    turn = () => new Promise((resolve) => setTimeout(resolve, 20));
  result.constructor = capture(() => new MessagePort());
  const channel = new MessageChannel();
  result.shape = {
    port: Object.prototype.toString.call(channel.port1),
    channel: Object.prototype.toString.call(channel),
    keys: Reflect.ownKeys(channel.port1),
    eventTarget: channel.port1 instanceof EventTarget,
  };
  result.cloneWithoutTransfer = capture(() => structuredClone(channel.port1));
  const ab = new ArrayBuffer(4),
    view = new Uint8Array(ab);
  view.set([1, 2, 3, 4]);
  const received = new Promise((resolve) => {
    channel.port2.onmessage = (e) =>
      resolve({
        bytes: Array.from(new Uint8Array(e.data.buffer)),
        alias: e.data.buffer === e.data.alias,
        ports: e.ports.length,
        trusted: e.isTrusted,
        origin: e.origin,
        source: e.source,
        phase: e.eventPhase,
      });
  });
  channel.port1.postMessage({ buffer: ab, alias: ab }, [ab]);
  result.portBufferDetached = ab.byteLength;
  result.portBuffer = await received;
  const queued = new MessageChannel();
  queued.port1.postMessage("one");
  queued.port1.postMessage("two");
  await turn();
  const moved = structuredClone(queued.port2, { transfer: [queued.port2] });
  const deliveries = [];
  moved.onmessage = (e) => deliveries.push(e.data);
  result.queueBeforeTurn = deliveries.slice();
  await turn();
  result.movedQueued = deliveries;
  result.retransferDetached = capture(() =>
    structuredClone(queued.port2, { transfer: [queued.port2] }),
  );
  result.detachedSend = capture(() => queued.port2.postMessage(() => {}));
  result.closedSend = (() => {
    const closed = new MessageChannel();
    closed.port1.close();
    return capture(() => closed.port1.postMessage(() => {}));
  })();
  const bad = new MessageChannel(),
    atomic = new ArrayBuffer(3);
  result.duplicateAtomic = capture(() =>
    bad.port1.postMessage({ buffer: atomic }, [atomic, atomic]),
  );
  result.duplicateLength = atomic.byteLength;
  result.selfTransfer = capture(() => bad.port1.postMessage("self", [bad.port1]));
  const failures = new ArrayBuffer(2);
  let getterCalls = 0;
  result.getterFailure = capture(() =>
    bad.port1.postMessage(
      {
        get value() {
          getterCalls++;
          throw new RangeError("boom");
        },
      },
      [failures],
    ),
  );
  result.getterFailureState = [getterCalls, failures.byteLength];
  const pair = new MessageChannel(),
    port = pair.port2,
    windowBuffer = new ArrayBuffer(2);
  new Uint8Array(windowBuffer).set([7, 8]);
  const windowDelivery = new Promise((resolve) =>
    window.addEventListener("message", function handler(e) {
      if (e.data?.kind !== "port-transfer") return;
      window.removeEventListener("message", handler);
      const delivered = e.ports[0];
      delivered.onmessage = (message) => {
        resolve({
          payloadPort: e.data.port === delivered,
          portRealm: Object.getPrototypeOf(delivered) === MessagePort.prototype,
          bytes: Array.from(new Uint8Array(e.data.buffer)),
          portsFrozen: Object.isFrozen(e.ports),
          reply: message.data,
          originMatches: e.origin === location.origin,
          source: e.source === window,
          trusted: e.isTrusted,
        });
      };
      pair.port1.postMessage("reply");
    }),
  );
  window.postMessage({ kind: "port-transfer", port, buffer: windowBuffer }, location.origin, [
    port,
    windowBuffer,
  ]);
  result.windowDetached = [
    windowBuffer.byteLength,
    capture(() => structuredClone(port, { transfer: [port] })),
  ];
  result.windowTransfer = await windowDelivery;
  const mismatch = new ArrayBuffer(5);
  let mismatchCalls = 0;
  window.postMessage(
    {
      get value() {
        mismatchCalls++;
        return 1;
      },
    },
    "https://mismatch.invalid",
    [mismatch],
  );
  result.originMismatch = [mismatch.byteLength, mismatchCalls];
  const closeTransferred = new MessageChannel();
  closeTransferred.port1.close();
  result.closedTransfer = capture(() => {
    const clone = structuredClone(closeTransferred.port1, { transfer: [closeTransferred.port1] });
    return clone instanceof MessagePort;
  });
  for (const value of [
    channel.port1,
    channel.port2,
    queued.port1,
    moved,
    bad.port1,
    bad.port2,
    pair.port1,
  ])
    value.close();
  return result;
})();
