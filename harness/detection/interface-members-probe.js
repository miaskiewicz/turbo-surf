(() => {
  // The interfaces of the expanded API inventory that were missing members against Chrome 156
  // (turbo-surf#165): every own prototype member with its descriptor kind, in key order.
  const NAMES = [
    "Animation",
    "CSSStyleRule",
    "CharacterData",
    "HTMLAnchorElement",
    "HTMLBodyElement",
    "HTMLDataListElement",
    "HTMLDivElement",
    "HTMLFieldSetElement",
    "HTMLHtmlElement",
    "HTMLIFrameElement",
    "HTMLImageElement",
    "HTMLLegendElement",
    "HTMLLinkElement",
    "HTMLMediaElement",
    "HTMLObjectElement",
    "HTMLOptGroupElement",
    "HTMLScriptElement",
    "HTMLStyleElement",
    "HTMLTemplateElement",
    "HTMLVideoElement",
    "XMLHttpRequest",
  ];
  const kind = (descriptor) => {
    if ("value" in descriptor) return typeof descriptor.value === "function" ? "method" : "value";
    return descriptor.set ? "accessor" : "getter";
  };
  const out = {};
  for (const name of NAMES) {
    const proto = globalThis[name]?.prototype;
    if (!proto) {
      out[name] = null;
      continue;
    }
    const members = {};
    for (const key of Object.getOwnPropertyNames(proto)) {
      if (key === "constructor") continue;
      members[key] = kind(Object.getOwnPropertyDescriptor(proto, key));
    }
    out[name] = members;
  }
  return JSON.stringify(out);
})();
