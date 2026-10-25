(async () => {
  for (let index = 0; index < 20 && !window.__log.some((row) => row[0] === "m2"); index++) await new Promise((done) => setTimeout(done, 10));
  const root = document.getElementById("h1").shadowRoot;
  const script = root.getElementById("s3");
  return {
    log: window.__log,
    states: [document.scripts.length, script.isConnected, script.getRootNode() === root, document.getElementById("h2").shadowRoot.querySelectorAll("script").length],
    rerun: (() => {
      const before = window.__log.length;
      root.append(script);
      const moved = document.createElement("div");
      moved.append(document.getElementById("h2").shadowRoot.firstChild);
      document.body.append(moved);
      return window.__log.length - before;
    })(),
  };
})()
