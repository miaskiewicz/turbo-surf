(async () => {
  const rows = [],
    retained = { policy: null, factory: null };
  for (const kind of ["released", "policy", "factory"]) {
    const frame = document.createElement("iframe");
    const loaded = new Promise((resolve) =>
      frame.addEventListener("load", resolve, { once: true }),
    );
    document.body.append(frame);
    // Each frame must finish initial navigation before its factory is measured.
    // eslint-disable-next-line no-await-in-loop
    await loaded;
    const child = frame.contentWindow,
      doc = child.document,
      factory = child.trustedTypes;
    const policy = factory.createPolicy("default", {
      createHTML(value) {
        return doc.nodeType === 9 ? value : "wrong-document";
      },
    });
    rows.push({ kind, refs: [child, doc, factory, policy].map((value) => new WeakRef(value)) });
    if (kind === "policy") retained.policy = policy;
    if (kind === "factory") retained.factory = factory;
    frame.remove();
  }
  globalThis.__trustedTypesLifetime = { rows, retained };
  globalThis.__readTrustedTypesLifetime = () => ({
    rows: rows.map(({ kind, refs }) => ({
      kind,
      alive: refs.map((ref) => ref.deref() !== undefined),
    })),
    retainedPolicyValue: globalThis.__trustedTypesLifetime.retained?.policy
      ? String(globalThis.__trustedTypesLifetime.retained.policy.createHTML("retained"))
      : null,
    retainedFactoryBrand: globalThis.__trustedTypesLifetime.retained?.factory
      ? globalThis.__trustedTypesLifetime.retained.factory.isHTML(
          globalThis.__trustedTypesLifetime.retained.factory.emptyHTML,
        )
      : null,
  });
  return "prepared";
})();
