(() => {
  const error = (fn) => {
    try {
      fn();
      return null;
    } catch (value) {
      return value.name;
    }
  };
  const script = document.createElement("script");
  script.textContent = "globalThis.__sandboxExecuted=true";
  document.body.append(script);
  const button = document.createElement("button");
  button.setAttribute("onclick", "globalThis.__sandboxHandler=true");
  document.body.append(button);
  button.click();
  const features = ["geolocation", "camera", "cross-origin-isolated"].map((name) => ({
    name,
    self: document.featurePolicy.allowsFeature(name),
    nullTarget: document.featurePolicy.allowsFeature(name, "null"),
    list: document.featurePolicy
      .getAllowlistForFeature(name)
      .map((value) =>
        value.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, (_, host) => `<${host}>`),
      ),
  }));
  let sharedClone = null;
  if (crossOriginIsolated) {
    const buffer = new SharedArrayBuffer(4);
    new Uint8Array(buffer)[0] = 9;
    const copy = structuredClone(buffer);
    new Uint8Array(copy)[0] = 11;
    sharedClone = {
      brand: Object.prototype.toString.call(copy),
      sameIdentity: copy === buffer,
      sharedByte: new Uint8Array(buffer)[0],
    };
  }
  const result = {
    sharedClone,
    features,
    origin: origin.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, (_, host) => `<${host}>`),
    locationOrigin: location.origin.replace(
      /http:\/\/(127\.0\.0\.1|localhost):\d+/g,
      (_, host) => `<${host}>`,
    ),
    secure: isSecureContext,
    isolated: crossOriginIsolated,
    script: globalThis.__sandboxExecuted === true,
    handler: globalThis.__sandboxHandler === true,
    cookie: error(() => {
      // oxlint-disable-next-line unicorn/no-document-cookie -- The control measures this exact native accessor.
      document.cookie = "sandboxTop=1;path=/";
      void document.cookie;
    }),
    storage: error(() => localStorage.setItem("sandboxTop", "1")),
  };
  script.remove();
  button.remove();
  return result;
})();
