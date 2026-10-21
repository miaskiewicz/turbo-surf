(async () => {
  const peer = document.createElement("iframe");
  const peerLoad = new Promise((resolve) => (peer.onload = resolve));
  document.body.append(peer);
  await peerLoad;
  let clearEvents = [];
  peer.contentWindow.addEventListener("storage", (event) =>
    clearEvents.push({
      key: event.key,
      url: event.url,
      area: event.storageArea === peer.contentWindow.localStorage ? "local" : "session",
      trusted: event.isTrusted,
    }),
  );
  const rootArea = { local: localStorage, session: sessionStorage };
  const read = () => ({
    local: rootArea.local.getItem("clear"),
    session: rootArea.session.getItem("clear"),
  });
  const seed = (value) => {
    rootArea.local.setItem("clear", value);
    rootArea.session.setItem("clear", value);
  };
  const out = {};
  const request = async (path, options) => {
    try {
      const response = await fetch(path, options);
      await response.text();
      return response.status;
    } catch (error) {
      return error.name;
    }
  };
  for (const [name, header, credentials] of [
    ["storage", '"storage"', "same-origin"],
    ["omit", '"storage"', "omit"],
    ["cacheOnly", '"cache"', "same-origin"],
    ["invalid", "storage", "same-origin"],
    ["wildcard", '"*"', "same-origin"],
  ]) {
    seed(name);
    await new Promise((resolve) => setTimeout(resolve, 20));
    clearEvents = [];
    const status = await request("/clear?case=" + name + "&header=" + encodeURIComponent(header), {
      credentials,
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    const clears = clearEvents.filter((event) => event.key === null);
    out[name] = {
      status,
      ...read(),
      same: rootArea.local === localStorage && rootArea.session === sessionStorage,
      clearEvents: {
        local: clears.filter((event) => event.area === "local"),
        session: clears.filter((event) => event.area === "session"),
      },
    };
  }
  rootArea.local.clear();
  rootArea.session.clear();
  await new Promise((resolve) => setTimeout(resolve, 20));
  clearEvents = [];
  for (const name of ["empty", "emptyAgain"]) {
    clearEvents = [];
    const status = await request(
      "/clear?case=" + name + "&header=" + encodeURIComponent('"storage"'),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    out[name] = {
      status,
      clearEvents: {
        local: clearEvents.filter((event) => event.area === "local"),
        session: clearEvents.filter((event) => event.area === "session"),
      },
    };
  }
  for (const kind of ["classic", "module", "style", "image"]) {
    seed(kind);
    const element = document.createElement(
      kind === "image" ? "img" : kind === "style" ? "link" : "script",
    );
    const loaded = new Promise((resolve) => {
      element.onload = element.onerror = resolve;
    });
    const url = "/resource-clear?kind=" + kind;
    if (kind === "style") {
      element.rel = "stylesheet";
      element.href = url;
    } else {
      if (kind === "module") element.type = "module";
      element.src = url;
    }
    document.body.append(element);
    await loaded;
    out[kind] = read();
    element.remove();
  }
  seed("before");
  await request("/clear?case=replay&header=" + encodeURIComponent('"storage"'));
  seed("after");
  out.replay = {
    status: await request("/clear?case=replay&header=" + encodeURIComponent('"storage"'), {
      cache: "force-cache",
    }),
    ...read(),
  };
  const frame = document.createElement("iframe");
  frame.src = "http://localhost:" + location.port + "/widget";
  const ready = new Promise((resolve) => {
    const listen = (event) => {
      if (event.source === frame.contentWindow && event.data.ready) {
        removeEventListener("message", listen);
        resolve();
      }
    };
    addEventListener("message", listen);
  });
  document.body.append(frame);
  await ready;
  let sequence = 0;
  const widget = (command) =>
    new Promise((resolve) => {
      const id = ++sequence,
        listen = (event) => {
          if (event.source === frame.contentWindow && event.data.id === id) {
            removeEventListener("message", listen);
            resolve(event.data.state);
          }
        };
      addEventListener("message", listen);
      frame.contentWindow.postMessage({ id, command }, "*");
    });
  for (const [name, cors, mode] of [
    ["corsAllowed", "allow", "cors"],
    ["corsDenied", "deny", "cors"],
    ["corpDenied", "deny", "no-cors"],
  ]) {
    await widget("seed");
    seed(name);
    const status = await request(
      "http://localhost:" +
        location.port +
        "/clear?case=" +
        name +
        "&header=" +
        encodeURIComponent('"storage"') +
        "&cors=" +
        cors +
        (name === "corpDenied" ? "&corp=same-origin" : ""),
      { credentials: "include", mode },
    );
    out[name] = { status, root: read(), child: await widget("read") };
  }
  await widget("seed");
  seed("widgetSelf");
  out.widgetSelf = { child: await widget("clear"), root: read() };
  const sameSiteFrame = document.createElement("iframe");
  sameSiteFrame.src = "http://127.0.0.1:" + globalThis.__storageClearSecondPort + "/widget";
  const sameSiteReady = new Promise((resolve) => {
    const listen = (event) => {
      if (event.source === sameSiteFrame.contentWindow && event.data.ready) {
        removeEventListener("message", listen);
        resolve();
      }
    };
    addEventListener("message", listen);
  });
  document.body.append(sameSiteFrame);
  await sameSiteReady;
  const sameSiteWidget = (command) =>
    new Promise((resolve) => {
      const id = ++sequence;
      const listen = (event) => {
        if (event.source === sameSiteFrame.contentWindow && event.data.id === id) {
          removeEventListener("message", listen);
          resolve(event.data.state);
        }
      };
      addEventListener("message", listen);
      sameSiteFrame.contentWindow.postMessage({ id, command }, "*");
    });
  for (const [name, cors, mode, credentials] of [
    ["sameSiteAllowed", "allow", "cors", "include"],
    ["sameSiteDenied", "deny", "cors", "include"],
    ["sameSiteCorpDenied", "deny", "no-cors", "include"],
    ["sameSiteOmit", "allow", "cors", "omit"],
  ]) {
    await sameSiteWidget("seed");
    const status = await request(
      "http://127.0.0.1:" +
        globalThis.__storageClearSecondPort +
        "/clear?case=" +
        name +
        "&header=%22storage%22&cors=" +
        cors +
        (name === "sameSiteCorpDenied" ? "&corp=same-origin" : ""),
      { mode, credentials },
    );
    out[name] = { status, child: await sameSiteWidget("read") };
  }
  for (const [name, origin, readWidget] of [
    ["iframeCrossSite", "http://localhost:" + location.port, widget],
    ["iframeSameSite", "http://127.0.0.1:" + globalThis.__storageClearSecondPort, sameSiteWidget],
  ]) {
    await readWidget("seed");
    const clearing = document.createElement("iframe");
    clearing.src = origin + "/widget?clearstorage=1";
    const loaded = new Promise((resolve) => {
      const listen = (event) => {
        if (event.source === clearing.contentWindow && event.data.ready) {
          removeEventListener("message", listen);
          resolve();
        }
      };
      addEventListener("message", listen);
    });
    document.body.append(clearing);
    await loaded;
    out[name] = await readWidget("read");
    clearing.remove();
  }
  sameSiteFrame.remove();
  frame.remove();
  peer.remove();
  return out;
})();
