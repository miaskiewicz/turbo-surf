(async () => {
  const item = globalThis.__policyFrameCase;
  const frame = document.createElement("iframe");
  const url = new URL(location.href);
  url.searchParams.set("child", "1");
  if (item.cross) url.hostname = location.hostname === "localhost" ? "127.0.0.1" : "localhost";
  if (item.allow) frame.setAttribute("allow", item.allow);
  if (item.credentialless) frame.credentialless = true;
  if (item.sandbox) frame.setAttribute("sandbox", item.sandbox);
  frame.src = url.href;
  const result = await new Promise((resolve) => {
    let received = null;
    const listener = (event) => {
      if (event.source === frame.contentWindow && event.data?.kind === "policy-frame")
        received = event.data;
    };
    addEventListener("message", listener);
    frame.onload = () =>
      setTimeout(() => {
        removeEventListener("message", listener);
        resolve({ child: received, accessible: frame.contentDocument !== null });
      }, 100);
    document.body.append(frame);
  });
  frame.remove();
  return { parent: { isolated: crossOriginIsolated, shared: typeof SharedArrayBuffer }, ...result };
})();
