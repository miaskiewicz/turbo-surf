(async () => {
  const item = globalThis.__policyFrameCase,
    frame = document.createElement("iframe");
  const url = new URL(location.href);
  url.searchParams.set("child", "1");
  if (item.cross) url.hostname = location.hostname === "localhost" ? "127.0.0.1" : "localhost";
  if (item.allow) frame.setAttribute("allow", item.allow);
  if (item.credentialless) frame.credentialless = true;
  frame.src = url.href;
  const result = {
    parentIsolated: crossOriginIsolated,
    child: null,
    parentSend: null,
    childSend: null,
    parentReceive: null,
    childReceive: null,
  };
  await new Promise((resolve) => {
    const listener = (event) => {
      if (event.source !== frame.contentWindow) return;
      if (Object.prototype.toString.call(event.data) === "[object SharedArrayBuffer]")
        result.parentReceive = "[object SharedArrayBuffer]";
      else if (event.data?.kind === "ready") {
        result.child = { isolated: event.data.isolated, shared: event.data.shared };
        try {
          frame.contentWindow.postMessage(
            {
              command: "shared",
              buffer: new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true }).buffer,
            },
            "*",
          );
          result.parentSend = "accepted";
        } catch (error) {
          result.parentSend = error.name;
        }
      } else if (event.data?.kind === "send") result.childSend = event.data.result;
      else if (event.data?.kind === "receive") result.childReceive = event.data.result;
    };
    const failed = (event) => {
      if (event.source === frame.contentWindow) result.parentReceive = "messageerror";
    };
    addEventListener("message", listener);
    addEventListener("messageerror", failed);
    frame.onload = () =>
      setTimeout(() => {
        removeEventListener("message", listener);
        removeEventListener("messageerror", failed);
        resolve();
      }, 200);
    document.body.append(frame);
  });
  frame.remove();
  return result;
})();
