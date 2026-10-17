(async () => {
  const frame = document.createElement("iframe");
  frame.srcdoc = "<!doctype html><body>first document</body>";
  const firstLoad = new Promise((resolve) =>
    frame.addEventListener("load", resolve, { once: true }),
  );
  document.body.appendChild(frame);
  await firstLoad;
  const previous = frame.contentDocument;
  const secondLoad = new Promise((resolve) =>
    frame.addEventListener("load", resolve, { once: true }),
  );
  frame.srcdoc = "<!doctype html><body>replacement document</body>";
  await secondLoad;
  const script = previous.createElement("script");
  script.textContent = "globalThis.retiredScriptExecuted=true";
  let error = null;
  try {
    previous.body.appendChild(script);
  } catch (exception) {
    error = exception.name;
  }
  const inserted = script.parentNode === previous.body;
  document.body.appendChild(script);
  return {
    error,
    inserted,
    adopted: script.ownerDocument === document,
    executed: globalThis.retiredScriptExecuted === true,
  };
})();
