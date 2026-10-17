(() => {
  const state = { rows: [], retained: [] };
  globalThis.__portLifetimeState = state;
  const row = (name, values) =>
    state.rows.push({ name, refs: values.map((value) => new WeakRef(value)) });
  {
    const node = document.createElement("section");
    node.expando = { connected: 17 };
    document.body.append(node);
    row("connected-wrapper-and-expando", [node, node.expando]);
    state.retained.push(node);
  }
  {
    const root = document.createElement("section"),
      first = document.createElement("p"),
      second = document.createElement("p");
    second.expando = { sibling: 19 };
    root.append(first, second);
    row("held-detached-descendant-retains-subtree", [root, first, second, second.expando]);
    state.retained.push(first);
  }
  {
    const root = document.createElement("section"),
      first = document.createElement("p"),
      second = document.createElement("p");
    second.expando = { split: 23 };
    root.append(first, second);
    second.remove();
    row("split-detached-subtree", [root, first, second, second.expando]);
    state.retained.push(first);
  }
  {
    const node = document.createElement("div");
    const callback = () => node;
    node.addEventListener("x", callback);
    row("unreachable-listener-node-cycle", [node, callback]);
  }
  {
    const node = document.createElement("div");
    node.expando = { style: 29 };
    const style = node.style;
    style.color = "red";
    row("retained-style-keeps-native-owner", [node, node.expando, style]);
    state.retained.push(style);
  }
  {
    const inert =
      globalThis.__authoredInertDocument ??
      new DOMParser().parseFromString("<body><section><p></p></section>", "text/html");
    delete globalThis.__authoredInertDocument;
    const root = inert.body.firstChild,
      node = root.firstChild;
    node.expando = { adopted: 31 };
    document.body.append(root);
    row("adopted-wrapper-releases-source-document", [inert, root, node, node.expando]);
    state.retained.push(root);
  }
  globalThis.__readPortLifetime = () =>
    state.rows.map((row) => ({
      name: row.name,
      alive: row.refs.map((ref) => ref.deref() !== undefined),
    }));
  const roots = state.retained;
  Object.defineProperty(state, "retained", {
    get() {
      return this.owners;
    },
    set(value) {
      this.owners = value;
      if (value === null) document.body.textContent = "";
    },
    configurable: true,
  });
  state.owners = roots;
})();
