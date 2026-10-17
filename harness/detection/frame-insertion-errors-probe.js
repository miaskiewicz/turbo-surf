(async () => {
  const outer = document.createElement("iframe");
  let loaded = new Promise((resolve) => {
    outer.onload = resolve;
  });
  document.body.append(outer);
  await loaded;
  const source = outer.contentDocument;
  const rows = {};
  const operations = {
    documentAppend: (frame) => document.append(frame, "text"),
    documentPrepend: (frame) => document.prepend(frame, "text"),
    documentReplaceChildren: (frame) => document.replaceChildren(frame, "text"),
    rootBefore: (frame) => document.documentElement.before(frame, "text"),
    rootAfter: (frame) => document.documentElement.after(frame, "text"),
    rootReplaceWith: (frame) => document.documentElement.replaceWith(frame, "text"),
    singleAppend: (frame) => document.appendChild(frame),
    invalidWebIDL: (frame) => document.append(frame, Symbol("invalid")),
    oneChildFragment: (frame) => {
      const fragment = source.createDocumentFragment();
      fragment.append(frame);
      document.append(fragment);
    },
  };
  try {
    for (const [name, operation] of Object.entries(operations)) {
      const holder = source.createElement("section"),
        frame = source.createElement("iframe");
      loaded = new Promise((resolve) => {
        frame.onload = resolve;
      });
      source.body.append(holder);
      holder.append(frame);
      await loaded;
      const oldWindow = frame.contentWindow,
        oldDocument = frame.contentDocument;
      oldDocument.body.textContent = "retained";
      let error = null;
      try {
        operation(frame);
      } catch (caught) {
        error = caught.name;
      }
      rows[name] = [
        error,
        frame.parentNode === null,
        frame.ownerDocument === source,
        holder.childNodes.length,
        oldWindow.closed,
        frame.contentWindow === null,
        oldDocument.defaultView === null,
        oldDocument.body.textContent,
      ];
      holder.remove();
    }
    return rows;
  } finally {
    outer.remove();
  }
})();
