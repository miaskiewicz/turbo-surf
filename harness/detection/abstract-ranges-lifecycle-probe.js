(async () => {
  const rows = [],
    retained = { range: null };
  for (const kind of ["released", "held"]) {
    const frame = document.createElement("iframe");
    const loaded = new Promise((resolve) =>
      frame.addEventListener("load", resolve, { once: true }),
    );
    document.body.append(frame);
    await loaded;
    const child = frame.contentWindow,
      doc = child.document,
      node = doc.createTextNode("retained source");
    doc.body.append(node);
    const range = new child.StaticRange({
      startContainer: node,
      startOffset: 2,
      endContainer: node,
      endOffset: 7,
    });
    rows.push({ kind, refs: [child, doc, node, range].map((value) => new WeakRef(value)) });
    if (kind === "held") retained.range = range;
    frame.remove();
  }
  globalThis.__authoredLifecycle = {
    read: () => ({
      rows: rows.map(({ kind, refs }) => ({
        kind,
        alive: refs.map((ref) => ref.deref() !== undefined),
      })),
      retained: retained.range
        ? [retained.range.startContainer.data, retained.range.startOffset, retained.range.endOffset]
        : null,
    }),
    release: () => {
      retained.range = null;
    },
  };
  return "prepared";
})();
