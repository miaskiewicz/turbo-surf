(async () => {
  const rows = [],
    retained = { validity: null, event: null };
  for (const kind of ["released", "validity", "event"]) {
    const frame = document.createElement("iframe"),
      loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
    document.body.append(frame);
    // eslint-disable-next-line no-await-in-loop
    await loaded;
    const child = frame.contentWindow,
      doc = child.document;
    doc.body.innerHTML =
      '<form><input required id="field"><button id="submitter">Send</button></form>';
    const input = doc.getElementById("field"),
      validity = input.validity,
      event = new child.SubmitEvent("submit", { submitter: input });
    rows.push({
      kind,
      refs: [child, doc, input, validity, event].map((value) => new WeakRef(value)),
    });
    if (kind === "validity") retained.validity = validity;
    if (kind === "event") retained.event = event;
    frame.remove();
  }
  globalThis.__formControlLifetime = { rows, retained };
  globalThis.__readFormControlLifetime = () => ({
    rows: rows.map(({ kind, refs }) => ({
      kind,
      alive: refs.map((ref) => ref.deref() !== undefined),
    })),
    retainedValidity: globalThis.__formControlLifetime.retained?.validity
      ? globalThis.__formControlLifetime.retained.validity.valueMissing
      : null,
    retainedSubmitter: globalThis.__formControlLifetime.retained?.event
      ? globalThis.__formControlLifetime.retained.event.submitter.id
      : null,
  });
  return "prepared";
})();
