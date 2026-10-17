(async () => {
  const high = String.fromCharCode(0xd800),
    low = String.fromCharCode(0xdc00);
  const script = document.createElement("script"),
    events = [];
  const executed = new Promise((resolve) => {
    globalThis.resolveModuleSource = resolve;
  });
  script.type = "module";
  script.textContent =
    "globalThis.moduleExact='" +
    high +
    "x" +
    low +
    "';globalThis.moduleFunction=function original(){return '" +
    high +
    "'};globalThis.moduleURL=import.meta.url;resolveModuleSource();";
  script.onload = () => events.push("load");
  script.onerror = () => events.push("error");
  document.body.append(script);
  await executed;
  await new Promise((resolve) => setTimeout(resolve, 0));
  delete globalThis.resolveModuleSource;
  return {
    units: Array.from({ length: moduleExact.length }, (_, index) => moduleExact.charCodeAt(index)),
    sourceUnit: moduleFunction.toString().charCodeAt(moduleFunction.toString().indexOf("'") + 1),
    sourceIdentity: moduleURL === document.URL,
    events,
  };
})();
