(async () => {
  const frame = document.createElement("iframe");
  const ready = new Promise((resolve) => (frame.onload = resolve));
  frame.srcdoc = "<!doctype html><body>";
  document.body.append(frame);
  await ready;
  frame.onload = null;
  const child = frame.contentWindow,
    doc = frame.contentDocument,
    descriptor = Object.getOwnPropertyDescriptor(child, "frameElement");
  const output = {
    descriptor: {
      get: typeof descriptor.get,
      set: typeof descriptor.set,
      configurable: descriptor.configurable,
      enumerable: descriptor.enumerable,
    },
    before: {
      owner: child.frameElement === frame,
      parent: child.parent === globalThis,
      closed: child.closed,
    },
  };
  let forged;
  try {
    descriptor.get.call({});
  } catch (error) {
    forged = error.name;
  }
  output.forged = forged;
  output.borrowedParent = descriptor.get.call(globalThis) === null;
  frame.remove();
  output.after = {
    ownerNull: child.frameElement === null,
    parentNull: child.parent === null,
    parentMain: child.parent === globalThis,
    closed: child.closed,
    defaultViewNull: doc.defaultView === null,
  };
  return output;
})();
