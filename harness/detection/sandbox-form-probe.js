(async () => {
  const frame = document.querySelector("iframe");
  const response = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Sandbox navigation child timed out")), 3000);
    const receive = (event) => {
      if (event.source !== frame.contentWindow || event.data?.kind !== "sandbox-form-result")
        return;
      clearTimeout(timer);
      removeEventListener("message", receive);
      resolve({
        clicks: event.data.clicks,
        submits: event.data.submits,
        trusted: event.data.trusted,
      });
    };
    addEventListener("message", receive);
    frame.contentWindow.postMessage("sandbox-form-run", "*");
  });
  return response;
})();
