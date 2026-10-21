(async () => {
  const result = {};
  const wait = () => new Promise((resolve) => setTimeout(resolve, 30));
  const rootClears = [];
  addEventListener("storage", (event) => {
    if (event.key === null)
      rootClears.push(event.storageArea === localStorage ? "local" : "session");
  });
  for (const kind of ["fetch", "classic", "module"]) {
    localStorage.setItem("seed", kind);
    sessionStorage.setItem("seed", kind);
    await wait();
    rootClears.length = 0;
    const peerClears = [];
    let frame;
    globalThis.registerClearPeer = () => {
      frame = document.createElement("iframe");
      document.body.append(frame);
      frame.contentWindow.addEventListener("storage", (event) => {
        if (event.key === null)
          peerClears.push(
            event.storageArea === frame.contentWindow.localStorage ? "local" : "session",
          );
      });
    };
    if (kind === "fetch") {
      const response = await fetch("/clear?header=%22storage%22");
      registerClearPeer();
      await response.text();
    } else {
      const script = document.createElement("script");
      if (kind === "module") script.type = "module";
      script.src = "/clear-register?kind=" + kind;
      const loaded = new Promise((resolve, reject) => {
        script.onload = resolve;
        script.onerror = reject;
      });
      document.body.append(script);
      await loaded;
      script.remove();
    }
    await wait();
    result[kind] = {
      root: rootClears.slice().sort(),
      peer: peerClears.slice().sort(),
      local: frame.contentWindow.localStorage.getItem("seed"),
      session: frame.contentWindow.sessionStorage.getItem("seed"),
    };
    frame.remove();
  }
  return result;
})();
