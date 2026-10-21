(() => {
  const send = (value) => parent.postMessage(value, "*");
  addEventListener("message", (event) => {
    if (event.data?.command === "shared")
      send({ kind: "receive", result: Object.prototype.toString.call(event.data.buffer) });
  });
  addEventListener("messageerror", () => send({ kind: "receive", result: "messageerror" }));
  send({ kind: "ready", isolated: crossOriginIsolated, shared: typeof SharedArrayBuffer });
  let result = "accepted";
  try {
    send(new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true }).buffer);
  } catch (error) {
    result = error.name;
  }
  send({ kind: "send", result });
})();
