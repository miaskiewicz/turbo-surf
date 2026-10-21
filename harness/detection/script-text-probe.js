(() => {
  const script = document.createElement("script"),
    source = "globalThis.__scriptTextExecuted=1";
  const descriptor = Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype, "text");
  script.text = source;
  const before = {
    text: script.text,
    content: script.textContent,
    children: script.childNodes.length,
    ownText: Object.hasOwn(script, "text"),
  };
  document.body.append(script);
  const result = {
    before,
    executed: globalThis.__scriptTextExecuted === 1,
    descriptor: descriptor
      ? {
          enumerable: descriptor.enumerable,
          configurable: descriptor.configurable,
          get: descriptor.get?.name,
          set: descriptor.set?.name,
        }
      : null,
  };
  script.remove();
  return result;
})();
