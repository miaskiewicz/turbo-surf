(async () => {
  "use strict";
  // turbo-surf#32: HTMLImageElement's src setter and decode() are prototype members, so an
  // <img> never carries own properties (Object.getOwnPropertyNames(img) is []), however it
  // was created, and assigning src or calling decode() still loads it.
  const P = HTMLImageElement.prototype;
  const png = document.getElementById("loaded").getAttribute("src");
  const broken = "data:image/png;base64,AAAA";
  const fn = (f) =>
    f && [f.name, f.length, Function.prototype.toString.call(f), Object.hasOwn(f, "prototype")];
  const settle = async (f) => {
    try {
      return ["ok", await f()];
    } catch (e) {
      return ["throw", e.name, e.message, e instanceof DOMException];
    }
  };
  const decode = Object.getOwnPropertyDescriptor(P, "decode");
  const src = Object.getOwnPropertyDescriptor(P, "src");
  const out = {};
  out.decode = {
    writable: decode.writable,
    enumerable: decode.enumerable,
    configurable: decode.configurable,
    value: fn(decode.value),
  };
  out.src = {
    enumerable: src.enumerable,
    configurable: src.configurable,
    get: fn(src.get),
    set: fn(src.set),
  };
  out.decodeReceivers = {};
  for (const [label, receiver] of Object.entries({
    plain: {},
    div: document.createElement("div"),
    prototype: P,
  })) {
    let returned = null;
    const outcome = await settle(() => {
      returned = P.decode.call(receiver);
      return returned.then(() => "resolved");
    });
    out.decodeReceivers[label] = [...outcome, returned instanceof Promise];
  }
  out.srcSetterReceiver = await settle(() => src.set.call({}, png));
  out.decodeEmpty = await settle(() =>
    document
      .createElement("img")
      .decode()
      .then(() => "resolved"),
  );
  out.decodeParsed = await settle(async () => {
    const img = document.getElementById("loaded");
    await img.decode();
    return [img.complete, img.naturalWidth, img.naturalHeight, Object.getOwnPropertyNames(img)];
  });
  const assigned = async (make, source) => {
    const img = make();
    const event = new Promise((done) => {
      img.addEventListener("load", () => done("load"), { once: true });
      img.addEventListener("error", () => done("error"), { once: true });
    });
    img.src = source;
    const decoded = await settle(() => img.decode().then(() => "resolved"));
    return {
      event: await event,
      decoded,
      complete: img.complete,
      naturalWidth: img.naturalWidth,
      naturalHeight: img.naturalHeight,
      currentSrc: img.currentSrc === source,
      own: Object.getOwnPropertyNames(img),
    };
  };
  out.created = await assigned(() => document.createElement("img"), png);
  out.createdBroken = await assigned(() => document.createElement("img"), broken);
  out.factory = await assigned(() => new Image(), png);
  out.innerHTML = await assigned(() => {
    const host = document.createElement("div");
    host.innerHTML = '<img alt="x">';
    return host.firstChild;
  }, png);
  out.documentOwnCreateElement = Object.hasOwn(document, "createElement");
  return out;
})();
