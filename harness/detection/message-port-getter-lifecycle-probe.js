(async () => {
  async function measure(mode) {
    const channel = new MessageChannel(),
      buffer = new ArrayBuffer(16),
      events = [];
    let moved = null,
      getterCalls = 0,
      error = null;
    channel.port2.onmessage = (event) => events.push(event.data.value);
    console.log(mode, "posting");
    globalThis.__getterControlStage = { mode, phase: "posting" };
    try {
      channel.port1.postMessage(
        {
          buffer,
          get value() {
            getterCalls++;
            console.log(mode, "getter");
            if (mode === "close-sender") channel.port1.close();
            if (mode === "transfer-sender")
              moved = structuredClone(channel.port1, { transfer: [channel.port1] });
            if (mode === "close-receiver") channel.port2.close();
            if (mode === "transfer-receiver")
              moved = structuredClone(channel.port2, { transfer: [channel.port2] });
            return mode;
          },
        },
        [buffer],
      );
    } catch (caught) {
      error = caught.name;
    }
    if (mode === "transfer-receiver") moved.onmessage = (event) => events.push(event.data.value);
    console.log(mode, "waiting", buffer.byteLength);
    globalThis.__getterControlStage = { mode, phase: "waiting", detached: buffer.byteLength };
    await new Promise((resolve) => setTimeout(resolve, 30));
    const result = { mode, error, getterCalls, detached: buffer.byteLength, events };
    console.log("result", JSON.stringify(result));
    channel.port1.close();
    channel.port2.close();
    moved?.close();
    return result;
  }
  const rows = [];
  for (const mode of globalThis.__portGetterModes || [
    "unchanged",
    "close-sender",
    "close-receiver",
    "transfer-receiver",
  ])
    rows.push(await measure(mode));
  return rows;
})();
