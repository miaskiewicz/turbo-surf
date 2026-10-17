(async () => {
  const results = [];
  for (const source of ["<p>plain</p>", "<template id=host><b>inert</b></template>"]) {
    const frame = document.createElement("iframe"),
      rows = [];
    const loaded = new Promise((resolve) => {
      const timeout = setTimeout(() => {
        rows.push(["timeout"]);
        resolve();
      }, 3000);
      frame.onload = (event) => {
        clearTimeout(timeout);
        rows.push([
          "load",
          event.isTrusted,
          event.target === frame,
          event.currentTarget === frame,
          frame.contentDocument.readyState,
        ]);
        resolve();
      };
    });
    frame.srcdoc = source;
    document.body.append(frame);
    await loaded;
    results.push(rows);
    frame.remove();
  }
  return results;
})();
