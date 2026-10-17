(async () => {
  const rows = [],
    retained = { values: [] };
  const load = async (owner, text) => {
    const frame = owner.createElement("iframe"),
      ready = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
    frame.srcdoc = `<!doctype html><body><p>${text}</p>`;
    owner.body.append(frame);
    await ready;
    return frame;
  };
  for (const mode of [
    "released-nested",
    "retained-nested-document",
    "retained-nested-function",
    "navigated-retained-nested-document",
  ]) {
    const outer = await load(document, "outer"),
      outerWindow = outer.contentWindow,
      outerDocument = outer.contentDocument,
      inner = await load(outerDocument, mode),
      innerWindow = inner.contentWindow,
      innerDocument = inner.contentDocument,
      node = innerDocument.querySelector("p"),
      read = innerWindow.Function("return document.querySelector('p').textContent");
    rows.push({
      mode,
      refs: [
        outer,
        outerDocument,
        outerWindow.Array,
        inner,
        innerDocument,
        innerWindow.Array,
        node,
        read,
      ].map((value) => new WeakRef(value)),
    });
    if (mode.includes("retained-nested-document")) retained.values.push(innerDocument);
    if (mode === "retained-nested-function") retained.values.push(read);
    if (mode.startsWith("navigated")) {
      const ready = new Promise((resolve) =>
        outer.addEventListener("load", resolve, { once: true }),
      );
      outer.srcdoc = "<!doctype html><body>replacement";
      await ready;
      retained.values.push(outer);
    } else outer.remove();
  }
  globalThis.__authoredLifecycle = {
    read: () => ({
      rows: rows.map(({ mode, refs }) => ({
        mode,
        alive: refs.map((ref) => ref.deref() !== undefined),
      })),
      retained: retained.values.map((value) =>
        typeof value === "function"
          ? ["function", value()]
          : value.nodeType === 9
            ? ["document", value.body.textContent, value.defaultView === null]
            : ["frame", value.contentDocument.body.textContent],
      ),
    }),
    release: () => {
      retained.values = [];
    },
  };
  return "prepared";
})();
