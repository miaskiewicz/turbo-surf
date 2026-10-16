(async () => {
  const frame = document.querySelector("iframe");
  const response = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Sandbox navigation child timed out")), 3000);
    const receive = (event) => {
      if (event.source !== frame.contentWindow || event.data?.kind !== "sandbox-navigation-result")
        return;
      clearTimeout(timer);
      removeEventListener("message", receive);
      resolve(event.data.error);
    };
    addEventListener("message", receive);
    frame.contentWindow.postMessage("sandbox-navigation-run", "*");
  });
  await new Promise((resolve) => setTimeout(resolve, 50));
  return { error: response, changed: location.hash === "#sandbox-navigation-check" };
})();
