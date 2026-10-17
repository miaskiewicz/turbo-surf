(async () => {
  const state = { rows: [], retained: null, hooks: 0 };
  globalThis.__portLifetimeState = state;
  for (const mode of ["expando", "prototype", "metadata-accessors", "hybrid-expando"]) {
    const key =
      mode === "hybrid-expando"
        ? (
            await crypto.subtle.generateKey("MLKEM768-X25519", false, [
              "encapsulateBits",
              "decapsulateBits",
            ])
          ).privateKey
        : await crypto.subtle.importKey("raw", new Uint8Array(16), "AES-GCM", false, ["encrypt"]);
    const channel = new MessageChannel();
    if (mode === "prototype") {
      Object.setPrototypeOf(
        key,
        Object.assign(Object.create(Object.getPrototypeOf(key)), { receiver: channel.port2 }),
      );
    } else if (mode === "metadata-accessors") {
      const metadata = { receiver: channel.port2 };
      for (const name of ["algorithm", "usages", "extractable", "type"]) {
        Object.defineProperty(key, name, {
          get() {
            state.hooks++;
            return metadata;
          },
        });
      }
    } else {
      key.receiver = channel.port2;
    }
    state.rows.push({
      name: mode,
      refs: [new WeakRef(channel.port1), new WeakRef(channel.port2), new WeakRef(key)],
    });
    channel.port1.postMessage({ key });
  }
  globalThis.__readPortLifetime = () =>
    state.rows.map((row) => ({
      name: row.name,
      alive: row.refs.map((ref) => ref.deref() !== undefined),
      hooks: state.hooks,
    }));
})();
