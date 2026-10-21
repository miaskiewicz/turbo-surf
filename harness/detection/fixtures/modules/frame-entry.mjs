import { owned } from "./frame-dep.mjs";
globalThis.entryRuns = (globalThis.entryRuns || 0) + 1;
globalThis.moduleResult = { owned, current: document.currentScript === null, url: import.meta.url };
document.title = "module child";
frameLog.push("module");
