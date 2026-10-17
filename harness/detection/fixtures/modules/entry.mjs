import { value, source } from "./dep.mjs";
globalThis.deferredModule = true;
globalThis.deferredEvidence = {
  value,
  source,
  entry: new URL(import.meta.url).pathname,
  currentScript: document.currentScript === null,
  ready: document.readyState,
};
moduleLog.push("deferred:eval");
