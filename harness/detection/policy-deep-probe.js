(async () => {
  const item = __policyFrameCase;
  const url = new URL(location.href);
  url.searchParams.set("child", "1");
  url.searchParams.set("depth", "1");
  url.searchParams.set("config", JSON.stringify(item));
  if (item.outerCross) url.hostname = "localhost";
  const frame = document.createElement("iframe");
  if (item.outerAllow !== null) frame.setAttribute("allow", item.outerAllow);
  const result = await new Promise((resolve) => {
    const listener = (event) => {
      if (event.source !== frame.contentWindow || event.data?.kind !== "policy-deep") return;
      window.removeEventListener("message", listener);
      resolve(event.data.result);
    };
    window.addEventListener("message", listener);
    frame.src = url.href;
    document.body.append(frame);
  });
  frame.remove();
  return result;
})();
