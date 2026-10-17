import { value } from "./late-dep.mjs";
globalThis.asyncModule = true;
globalThis.asyncEvidence = {
  value,
  currentScript: document.currentScript === null,
  ready: document.readyState,
};
moduleLog.push("async:eval");
