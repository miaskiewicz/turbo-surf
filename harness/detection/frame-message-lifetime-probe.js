(async () => {
  const state = { rows: [], retained: [], events: [] };
  globalThis.__portLifetimeState = state;
  const load = async () => {
    const frame = document.createElement("iframe");
    const ready = new Promise((resolve) => (frame.onload = resolve));
    frame.srcdoc = "<!doctype html><body><script>window.marker={};</script>";
    document.body.appendChild(frame);
    await ready;
    frame.onload = null;
    return frame;
  };
  for (const mode of [
    "removed-unretained",
    "removed-retained-window",
    "removed-retained-document",
    "navigated-unretained-old-document",
    "navigated-retained-old-document",
  ]) {
    const frame = await load(),
      window = frame.contentWindow,
      document = frame.contentDocument;
    const refs = [
      new WeakRef(frame),
      new WeakRef(document),
      new WeakRef(window.Array),
      new WeakRef(window.marker),
    ];
    if (mode === "removed-retained-window") state.retained.push(window);
    if (mode.endsWith("retained-document") || mode === "navigated-retained-old-document")
      state.retained.push(document);
    if (mode.startsWith("navigated")) {
      const ready = new Promise((resolve) => (frame.onload = resolve));
      frame.srcdoc = "<!doctype html><body>replacement";
      await ready;
      frame.onload = null;
      state.retained.push(frame);
    } else frame.remove();
    state.rows.push({ name: mode, refs });
  }
  {
    const frame = await load(),
      child = frame.contentWindow;
    const channel = new child.MessageChannel();
    const parentPort = structuredClone(channel.port1, { transfer: [channel.port1] });
    parentPort.onmessage = (event) =>
      state.events.push(["removed-child-source-parent-target", event.data]);
    state.retained.push(parentPort, channel.port2);
    frame.remove();
    channel.port2.postMessage("after-removal");
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
  {
    const frame = await load(),
      child = frame.contentWindow;
    const channel = new child.MessageChannel();
    state.retained.push(channel.port1, channel.port2);
    channel.port1.onmessage = (event) =>
      state.events.push(["removed-retained-source-port", event.data]);
    frame.remove();
    channel.port2.postMessage("after-removal");
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
  {
    const frame = await load(),
      child = frame.contentWindow;
    const channel = new MessageChannel();
    child.received = [];
    child.addEventListener("message", (event) => {
      child.retainedPort = event.ports[0];
      child.retainedPort.onmessage = (message) =>
        parent.__portLifetimeState.events.push(["transferred-child-removed", message.data]);
      parent.__portLifetimeState.events.push(["transferred-child-ready", event.ports.length]);
    });
    child.postMessage("transfer", "*", [channel.port2]);
    await new Promise((resolve) => setTimeout(resolve, 30));
    frame.remove();
    channel.port1.postMessage("after-removal");
    await new Promise((resolve) => setTimeout(resolve, 30));
    state.retained.push(channel.port1);
  }
  {
    const frame = await load(),
      child = frame.contentWindow,
      buffer = new ArrayBuffer(4);
    frame.remove();
    let error = null;
    try {
      child.postMessage({ buffer }, "*", [buffer]);
    } catch (caught) {
      error = caught.name;
    }
    state.events.push(["removed-window-transfer", buffer.byteLength, error]);
  }
  {
    const frame = await load(),
      child = frame.contentWindow;
    const received = (event) => {
      if (event.data === "queued-child-before-removal")
        state.events.push(["parent-message-after-child-removal", event.source !== null]);
    };
    addEventListener("message", received);
    child.eval("parent.postMessage('queued-child-before-removal','*')");
    frame.remove();
    await new Promise((resolve) => setTimeout(resolve, 30));
    removeEventListener("message", received);
  }
  {
    const frame = await load(),
      child = frame.contentWindow;
    child.eval(
      "(()=>{const marker={};parent.__retiredTimerPayload=new parent.WeakRef(marker);setTimeout(()=>marker,1000);})()",
    );
    state.retained.push(frame.contentDocument);
    const refs = [globalThis.__retiredTimerPayload, new WeakRef(frame.contentDocument)];
    frame.remove();
    state.rows.push({ name: "removed-retained-document-timer-payload", refs });
  }
  globalThis.__readPortLifetime = () => ({
    rows: state.rows.map((row) => ({
      name: row.name,
      alive: row.refs.map((ref) => ref.deref() !== undefined),
    })),
    events: state.events,
  });
})();
