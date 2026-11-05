(async () => {
  "use strict";
  // Runs after a csp-directives case document loaded. The document's first (nonced)
  // script records every securitypolicyviolation in `__violations` and its parser-inserted
  // scripts record themselves in `__ran`; this probe adds the dynamic requests.
  const port = location.port;
  const cross = `http://localhost:${port}`;
  const ran = () => window.__ran || (window.__ran = []);
  const script = (src) =>
    new Promise((done) => {
      const element = document.createElement("script");
      if (src) {
        element.src = src;
        element.onload = () => done("load");
        element.onerror = () => done("error");
      } else element.textContent = "(window.__ran||(window.__ran=[])).push('dynamic-inline')";
      document.body.append(element);
      if (!src) done("inline");
    });
  const fetched = (url) =>
    fetch(url).then(
      (response) => response.status,
      (error) => error.name,
    );
  const socket = (url) => {
    try {
      const ws = new WebSocket(url);
      ws.onerror = () => {};
      return "created";
    } catch (error) {
      return error.name;
    }
  };
  const out = {};
  out.dynamicSame = await script("/s/dynamic-same.js");
  out.dynamicCross = await script(`${cross}/s/dynamic-cross.js`);
  out.dynamicInline = await script("");
  const button = document.createElement("button");
  button.setAttribute("onclick", "(window.__ran||(window.__ran=[])).push('attribute')");
  document.body.append(button);
  button.click();
  out.fetchSame = await fetched("/f/same?q=1#fragment");
  out.fetchCross = await fetched(`${cross}/f/cross?q=1#fragment`);
  out.sockets = [
    socket(`ws://127.0.0.1:${port}/ws/same`),
    socket(`ws://localhost:${port}/ws/cross`),
    socket(`wss://127.0.0.1:${port}/ws/secure`),
    socket(`wss://localhost:${port}/ws/secure-cross`),
  ];
  const image = new Image();
  image.src = `${cross}/i/cross.png?q=1#fragment`;
  document.body.append(image);
  // Workers and media are gated by worker-src / media-src only where the case opts in.
  if (document.body.dataset.extra === "worker-media") {
    try {
      new Worker("/s/worker.js");
      out.worker = "created";
    } catch (error) {
      out.worker = error.name;
    }
    const audio = document.createElement("audio");
    audio.src = "/m/audio.mp3";
    document.body.append(audio);
  }
  await new Promise((resolve) => setTimeout(resolve, 300));
  out.ran = ran().slice();
  out.violations = (window.__violations || []).map((row) =>
    row.map((value) => String(value).replaceAll(`:${port}`, ":<port>")),
  );
  return out;
})();
