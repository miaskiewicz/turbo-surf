(async () => {
  const rows = [],
    retained = { internals: null, states: null, iterator: null, doneIterator: null };
  for (const kind of ["released", "internals", "states", "iterator", "doneIterator"]) {
    const frame = document.createElement("iframe"),
      loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
    document.body.append(frame);
    // eslint-disable-next-line no-await-in-loop
    await loaded;
    const child = frame.contentWindow,
      doc = child.document;
    const objects = child.eval(
      "(()=>{customElements.define('x-lifetime-internals',class extends HTMLElement{constructor(){super();this.internals=this.attachInternals();this.internals.states.add('ready');}});const host=document.createElement('x-lifetime-internals');document.body.append(host);const internals=host.internals,states=internals.states,iterator=states.values();return [host,internals,states,iterator];})()",
    );
    const [host, internals, states, iterator] = objects;
    if (kind === "doneIterator") {
      iterator.next();
      iterator.next();
    }
    rows.push({
      kind,
      refs: [child, doc, host, internals, states, iterator].map((value) => new WeakRef(value)),
    });
    if (kind === "internals") retained.internals = internals;
    if (kind === "states") retained.states = states;
    if (kind === "iterator") retained.iterator = iterator;
    if (kind === "doneIterator") retained.doneIterator = iterator;
    frame.remove();
  }
  globalThis.__internalsLifetime = { rows, retained };
  globalThis.__readInternalsLifetime = () => ({
    rows: rows.map(({ kind, refs }) => ({
      kind,
      alive: refs.map((ref) => ref.deref() !== undefined),
    })),
    internalsReady: globalThis.__internalsLifetime.retained?.internals?.states.has("ready") ?? null,
    statesReady: globalThis.__internalsLifetime.retained?.states?.has("ready") ?? null,
    iteratorNext: globalThis.__internalsLifetime.retained?.iterator?.next() ?? null,
    doneNext: globalThis.__internalsLifetime.retained?.doneIterator?.next() ?? null,
  });
  return "prepared";
})();
