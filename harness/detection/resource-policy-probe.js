(async () => {
  const cross = document.querySelector("meta[name=fixture-cross]").getAttribute("content");
  const hash = document.querySelector("meta[name=fixture-data-integrity]").getAttribute("content");
  const result = { fetch: {}, resources: {}, events: {} };
  for (const [name, url, options] of [
    ["raw", "/data", { integrity: hash }],
    ["tampered", "/data", { integrity: "sha256-YQ==" }],
    ["strongest", "/data", { integrity: `${hash} sha512-YQ==` }],
    ["unknown", "/data", { integrity: "unknown-YQ==" }],
    ["cors", `${cross}/data`, { integrity: hash }],
    ["opaque", `${cross}/data`, { mode: "no-cors", integrity: hash }],
    ["opaqueUnknown", `${cross}/data`, { mode: "no-cors", integrity: "unknown-YQ==" }],
  ]) {
    try {
      result.fetch[name] = Array.from(
        new Uint8Array(await (await fetch(url, options)).arrayBuffer()),
      );
    } catch (error) {
      result.fetch[name] = { error: error.name };
    }
  }
  result.corp = {};
  for (const [name, mode] of [
    ["origin", "no-cors"],
    ["site", "no-cors"],
    ["cross", "no-cors"],
    ["invalid", "no-cors"],
    ["duplicate", "no-cors"],
    ["origin", "cors"],
  ]) {
    try {
      result.corp[`${name}/${mode}`] = (await fetch(`${cross}/corp-${name}.js`, { mode })).type;
    } catch (error) {
      result.corp[`${name}/${mode}`] = { error: error.name };
    }
  }
  for (const id of ["valid-style", "blocked-style"]) {
    try {
      result.resources[id] = document.getElementById(id).sheet?.cssRules.length ?? null;
    } catch (error) {
      result.resources[id] = { error: error.name };
    }
  }
  result.resources.script = globalThis.integrityScript ?? null;
  result.resources.module = globalThis.integrityModule ?? null;
  result.resources.dependency = globalThis.integrityDependency ?? false;
  const child = document.querySelector("iframe").contentWindow;
  result.resources.childScript = child.integrityScript ?? null;
  result.resources.childErrors = child.integrityErrors;
  for (const [id, type, trust] of globalThis.resourceEvents) result.events[id] = [type, trust];
  const encoded = JSON.stringify(result);
  globalThis.__RESULT = encoded;
  return encoded;
})();
