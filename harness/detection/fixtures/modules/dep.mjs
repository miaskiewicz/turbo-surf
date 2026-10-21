export const value = "linked";
export const source = {
  url: new URL(import.meta.url).pathname,
  currentScript: document.currentScript === null,
  stack: new Error("module-source").stack.includes("/dep.mjs"),
};
