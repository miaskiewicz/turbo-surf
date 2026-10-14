(async () => {
  const frame = document.createElement("iframe");
  const loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
  frame.srcdoc = "<!doctype html><body>authored inherited embedder policy</body>";
  document.body.appendChild(frame);
  await loaded;
  const ownRealm = frame.contentWindow.Array !== Array;
  try {
    const response = await frame.contentWindow.fetch(
      __coepPolicies.cross + "/echo?case=0&mode=no-cors",
      { mode: "no-cors", credentials: "include" },
    );
    return { ownRealm, allowed: true, type: response.type, status: response.status };
  } catch (error) {
    return {
      ownRealm,
      allowed: false,
      name: error.name,
      errorRealm: error instanceof frame.contentWindow.TypeError,
    };
  }
})();
