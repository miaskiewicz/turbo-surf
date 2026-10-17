/* eslint-disable unicorn/consistent-function-scoping, unicorn/prefer-add-event-listener -- The oracle is a self-contained expression evaluated in the page realm. This control intentionally exercises the iframe onload IDL handler. */
(async () => {
  const out = {};
  const timed = async (promise) =>
    Promise.race([
      promise.then(
        (value) => ({ settled: true, value }),
        (error) => ({ settled: true, error: { name: error.name, message: error.message } }),
      ),
      new Promise((resolve) => setTimeout(() => resolve({ settled: false }), 500)),
    ]);
  const frame = async () => {
    const item = document.createElement("iframe");
    const loaded = new Promise((resolve) => (item.onload = resolve));
    item.src = "/stream-frame";
    document.body.append(item);
    await loaded;
    return item;
  };
  {
    const item = await frame(),
      child = item.contentWindow;
    const source = new child.ReadableStream({
      start: new child.Function("value", "globalThis.__controller=value"),
    });
    const controller = child.__controller;
    const target = structuredClone(source, { transfer: [source] });
    const reader = target.getReader();
    item.remove();
    controller.enqueue(42);
    controller.close();
    out.readableAfterRetirement = await timed(reader.read());
  }
  {
    const item = await frame(),
      child = item.contentWindow;
    const accepted = [];
    child.__accepted = accepted;
    const source = new child.WritableStream({
      write: new child.Function("value", "globalThis.__accepted.push(value)"),
    });
    const target = structuredClone(source, { transfer: [source] });
    const writer = target.getWriter();
    item.remove();
    out.writableAfterRetirement = await timed(writer.write(43));
    await new Promise((resolve) => setTimeout(resolve, 20));
    out.writableAccepted = accepted;
  }
  {
    const item = await frame(),
      child = item.contentWindow;
    const source = new ReadableStream({
      start(controller) {
        controller.enqueue(44);
        controller.close();
      },
    });
    const target = child.structuredClone(source, { transfer: [source] });
    const reader = target.getReader();
    item.remove();
    out.receiverAfterRetirement = await timed(reader.read());
  }
  return out;
})();
