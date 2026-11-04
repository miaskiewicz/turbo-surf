(async () => {
  // @font-face and the CSS Font Loading API (turbo-surf#130): CSS-connected faces
  // fetched as the document uses them (ttf/woff/woff2, src fallback, CORS, CSP
  // font-src), document.fonts status/ready/events timing, FontFace from a URL and
  // from bytes, and the geometry/measureText a loaded face gives.
  const root = document.documentElement;
  const cross = root.dataset.cross;
  const fonts = document.fonts;
  const events = [];
  const order = [];
  for (const type of ["loading", "loadingdone", "loadingerror"])
    fonts.addEventListener(type, (event) =>
      events.push([type, Array.from(event.fontfaces, (face) => face.family).sort()]),
    );
  const faces = () =>
    Array.from(fonts, (face) => [face.family, face.status]).sort((a, b) =>
      a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0,
    );
  const width = (id) => document.getElementById(id).getBoundingClientRect().width;
  const out = {
    initial: { size: fonts.size, status: fonts.status, tag: Object.prototype.toString.call(fonts) },
  };
  const firstReady = fonts.ready;
  const sources = {
    TTF: "url(/fonts/plex.ttf) format('truetype')",
    WOFF: "url(/fonts/plex.woff) format('woff')",
    WOFF2: "url(/fonts/plex.woff2) format('woff2')",
    Fallback: "url(/fonts/missing.ttf), url(/fonts/plex.ttf)",
    Missing: "url(/fonts/missing.ttf)",
    CrossCors: `url(${cross}/fonts/plex.ttf?cors=1)`,
    CrossOpaque: `url(${cross}/fonts/plex.ttf)`,
    Unused: "url(/fonts/plex.ttf)",
  };
  const used = Object.keys(sources).filter((name) => name !== "Unused");
  const style = document.createElement("style");
  style.textContent =
    Object.entries(sources)
      .map(([name, src]) => `@font-face{font-family:"Surf ${name}";src:${src}}`)
      .join("") + used.map((name) => `#t${name}{font:20px/24px "Surf ${name}",monospace}`).join("");
  document.head.append(style);
  for (const name of used) {
    const span = document.createElement("span");
    span.id = `t${name}`;
    span.textContent = "Hamburgefonstiv 0123";
    document.body.append(span, document.createElement("br"));
  }
  out.inserted = { size: fonts.size, status: fonts.status, faces: faces() };
  void document.body.offsetWidth;
  out.laidOut = {
    status: fonts.status,
    faces: faces(),
    readyReplaced: fonts.ready !== firstReady,
    events: events.slice(),
  };
  fonts.ready.then(() => order.push("ready"));
  await fonts.ready;
  const ctx = document.createElement("canvas").getContext("2d");
  const measure = (family) => {
    ctx.font = `20px "Surf ${family}"`;
    return ctx.measureText("Hamburgefonstiv 0123").width;
  };
  out.ready = {
    status: fonts.status,
    faces: faces(),
    events: events.slice(),
    widths: Object.fromEntries(
      ["TTF", "WOFF", "WOFF2", "Fallback"].map((name) => [name, width(`t${name}`)]),
    ),
    heights: Object.fromEntries(
      ["TTF", "WOFF2"].map((name) => [
        name,
        document.getElementById(`t${name}`).getBoundingClientRect().height,
      ]),
    ),
    canvas: Object.fromEntries(["TTF", "WOFF2"].map((name) => [name, measure(name)])),
    check: {
      loaded: fonts.check('20px "Surf TTF"'),
      unloaded: fonts.check('20px "Surf Unused"'),
      unknown: fonts.check('20px "Surf Nowhere"'),
    },
  };
  events.length = 0;
  order.length = 0;

  // FontFace from a URL: unloaded until load(); load() starts it synchronously.
  const face = new FontFace("Surf Script", "url(/fonts/plex.woff2)", { weight: "400" });
  const constructed = {
    status: face.status,
    family: face.family,
    weight: face.weight,
    style: face.style,
  };
  fonts.add(face);
  const added = { size: fonts.size, status: fonts.status, faceStatus: face.status };
  const before = fonts.ready;
  const loading = face.load();
  const started = {
    faceStatus: face.status,
    setStatus: fonts.status,
    samePromise: loading === face.loaded,
    readyReplaced: fonts.ready !== before,
    events: events.slice(),
  };
  loading.then(() => order.push("load"));
  fonts.ready.then(() => order.push("ready"));
  Promise.resolve().then(() => order.push("microtask"));
  setTimeout(() => order.push("task"), 0);
  await loading;
  order.push("awaited");
  await fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 20));
  const probe = document.createElement("span");
  probe.id = "tScript";
  probe.style.font = '20px/24px "Surf Script",monospace';
  probe.textContent = "Hamburgefonstiv 0123";
  document.body.append(probe);
  out.script = {
    constructed,
    added,
    started,
    loaded: { faceStatus: face.status, setStatus: fonts.status },
    order: order.slice(),
    events: events.slice(),
    width: width("tScript"),
  };
  events.length = 0;

  // FontFace from bytes: parsed without a network load.
  const bytes = await (await fetch("/fonts/plex.ttf")).arrayBuffer();
  const buffered = new FontFace("Surf Bytes", bytes);
  const bufferedStatus = buffered.status;
  await buffered.loaded;
  fonts.add(buffered);
  const bytesSpan = document.createElement("span");
  bytesSpan.id = "tBytes";
  bytesSpan.style.font = '20px/24px "Surf Bytes",monospace';
  bytesSpan.textContent = "Hamburgefonstiv 0123";
  document.body.append(bytesSpan);
  const invalid = new FontFace("Surf Invalid", new Uint8Array([1, 2, 3]));
  const invalidStatus = invalid.status;
  const invalidError = await invalid.loaded.then(
    () => "resolved",
    (error) => error.name,
  );
  const badDescriptor = new FontFace("Surf Bad", "url(/fonts/plex.ttf)", { weight: "bogus" });
  const badError = await badDescriptor.loaded.then(
    () => "resolved",
    (error) => error.name,
  );
  out.bytes = {
    status: bufferedStatus,
    loadedStatus: buffered.status,
    width: width("tBytes"),
    invalid: [invalidStatus, invalid.status, invalidError],
    badDescriptor: [badDescriptor.status, badError],
  };

  // document.fonts.load() loads the matching unloaded CSS face.
  const loaded = await fonts.load('20px "Surf Unused"');
  out.setLoad = {
    families: loaded.map((item) => item.family),
    status: Array.from(fonts).find((item) => item.family === "Surf Unused").status,
    events: events.slice(),
  };
  fonts.delete(face);
  out.deleted = { size: fonts.size, has: fonts.has(face) };
  return JSON.stringify(out);
})();
