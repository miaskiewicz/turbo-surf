(async () => {
  globalThis.__orderedScriptRows = [];
  const finished = [];
  for (const id of ["slow", "fast"]) {
    const script = document.createElement("script");
    script.id = id;
    script.async = false;
    script.src = "/" + id + ".js";
    finished.push(
      new Promise((resolve, reject) => {
        script.onload = () => {
          __orderedScriptRows.push("load:" + id);
          resolve();
        };
        script.onerror = () => reject(new Error("authored resource failed: " + id));
      }),
    );
    document.head.appendChild(script);
    __orderedScriptRows.push("append:" + id + ":" + (document.currentScript === null));
  }
  await Promise.all(finished);
  return __orderedScriptRows;
})();
