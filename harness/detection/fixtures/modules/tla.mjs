globalThis.moduleTlaStarted = true;
moduleLog.push("tla:start");
await fetch("/await-gate");
globalThis.moduleTlaDone = true;
globalThis.moduleTlaResult = {
  currentScript: document.currentScript === null,
  readyState: document.readyState,
  url: new URL(import.meta.url).pathname,
};
moduleLog.push("tla:end");
finishModuleProbe();
