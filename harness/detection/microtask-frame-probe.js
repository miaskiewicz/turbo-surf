(async () => {
  const rows = [];
  const frame = document.createElement("iframe");
  const loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
  document.body.append(frame);
  await loaded;
  const child = frame.contentWindow;
  globalThis.microtaskFrameRows = rows;
  const callback = child.Function("parent.microtaskFrameRows.push('child-callback')");
  child.queueMicrotask(() => rows.push("owner-child"));
  queueMicrotask(callback);
  await new Promise((resolve) => queueMicrotask(resolve));
  const before = rows.splice(0);
  child.queueMicrotask(() => rows.push("retired-owner"));
  queueMicrotask(callback);
  frame.remove();
  await new Promise((resolve) => queueMicrotask(resolve));
  return { before, retired: rows.slice() };
})();
