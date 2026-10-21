(() => {
  const state = { rows: [], retained: null };
  globalThis.__portLifetimeState = state;
  function row(name, make) {
    const channel = new MessageChannel();
    const refs = [new WeakRef(channel.port1), new WeakRef(channel.port2)];
    make(channel);
    state.rows.push({ name, refs });
  }
  row("unreferenced-open-pair", () => {});
  row("unreferenced-listening-pair", (channel) => {
    channel.port2.onmessage = () => {};
  });
  row("unreferenced-closed-pair", (channel) => {
    channel.port1.close();
    channel.port2.close();
  });
  row("reachable-peer", (channel) => {
    state.retained = [channel.port1];
    channel.port2.onmessage = () => {};
  });
  row("reachable-peer-without-listener", (channel) => {
    state.retained.push(channel.port1);
  });
  {
    const first = new MessageChannel(),
      second = new MessageChannel();
    const refs = [new WeakRef(first.port1), new WeakRef(second.port1)];
    second.port1.postMessage(first.port2, [first.port2]);
    first.port1.postMessage(second.port2, [second.port2]);
    state.rows.push({ name: "unreachable-queued-transfer-cycle", refs });
  }
  {
    const first = new MessageChannel(),
      second = new MessageChannel();
    const refs = [new WeakRef(first.port1), new WeakRef(second.port2)];
    first.port1.onmessage = () => {};
    second.port1.postMessage(first.port2, [first.port2]);
    state.retained.push(second.port2);
    state.rows.push({ name: "reachable-queued-transfer-peer", refs });
  }
  globalThis.__readPortLifetime = () =>
    state.rows.map((row) => ({
      name: row.name,
      alive: row.refs.map((ref) => ref.deref() !== undefined),
    }));
})();
