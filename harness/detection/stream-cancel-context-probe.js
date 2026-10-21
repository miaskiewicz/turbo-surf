/* eslint-disable unicorn/consistent-function-scoping, no-await-in-loop -- The oracle is a self-contained expression evaluated in the page realm. Sequential cancel acknowledgement is the behavior being measured. */
(async () => {
  const out = {};
  for (const name of ["stream", "reader", "writer"]) {
    let received;
    const source =
      name === "writer"
        ? new WritableStream({
            abort(e) {
              received = e;
            },
          })
        : new ReadableStream({
            cancel(e) {
              received = e;
            },
          });
    const copy = structuredClone(source, { transfer: [source] });
    try {
      await (name === "stream"
        ? copy.cancel(() => {})
        : name === "reader"
          ? copy.getReader().cancel(() => {})
          : copy.getWriter().abort(() => {}));
    } catch (e) {
      out[name] = { name: e.name, message: e.message };
    }
    await new Promise((r) => setTimeout(r, 10));
    out[name].source = { name: received?.name, message: received?.message };
  }
  return out;
})();
