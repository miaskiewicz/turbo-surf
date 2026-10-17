(async () => {
  const result = { global: typeof SharedArrayBuffer, isolated: crossOriginIsolated };
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  result.wasm = {
    brand: Object.prototype.toString.call(memory.buffer),
    bytes: memory.buffer.byteLength,
  };
  const outcome = (operation) => {
    try {
      operation();
      return "accepted";
    } catch (error) {
      return error.name;
    }
  };
  result.clone = outcome(() => structuredClone(memory.buffer));
  const regular = new ArrayBuffer(4);
  result.atomic = outcome(() =>
    structuredClone({ regular, shared: memory.buffer }, { transfer: [regular] }),
  );
  result.retained = regular.byteLength;
  const channel = new MessageChannel();
  result.port = outcome(() => channel.port1.postMessage(memory.buffer));
  result.window = outcome(() => postMessage(memory.buffer, "*"));
  channel.port1.close();
  channel.port2.close();
  Object.defineProperty(globalThis, "crossOriginIsolated", { value: true, configurable: true });
  globalThis.SharedArrayBuffer = memory.buffer.constructor;
  result.forged = {
    flag: crossOriginIsolated,
    constructor: typeof SharedArrayBuffer,
    clone: outcome(() => structuredClone(memory.buffer)),
  };
  return result;
})();
