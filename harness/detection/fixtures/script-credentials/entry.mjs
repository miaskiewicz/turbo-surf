import { value } from "./dep.mjs";
globalThis.credentialModule = {
  value,
  current: document.currentScript === null,
  url: new URL(import.meta.url).pathname,
};
