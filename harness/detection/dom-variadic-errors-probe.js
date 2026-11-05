(async () => {
  const frame = document.createElement("iframe");
  const loaded = new Promise((resolve) => {
    frame.onload = resolve;
  });
  document.body.append(frame);
  await loaded;
  const source = frame.contentDocument;
  const rows = {};
  const operations = {
    documentAppend: (node) => document.append(node, "text"),
    documentPrepend: (node) => document.prepend(node, "text"),
    documentReplaceChildren: (node) => document.replaceChildren(node, "text"),
    rootBefore: (node) => document.documentElement.before(node, "text"),
    rootAfter: (node) => document.documentElement.after(node, "text"),
    rootReplaceWith: (node) => document.documentElement.replaceWith(node, "text"),
  };
  const caughtName = (run) => {
    try {
      run();
      return null;
    } catch (caught) {
      return caught.name;
    }
  };
  const operationRow = (operation) => {
    const holder = source.createElement("section");
    const node = source.createElement("b");
    node.textContent = "foreign";
    holder.append(node);
    source.body.append(holder);
    const root = document.documentElement;
    const range = source.createRange();
    range.selectNodeContents(node.firstChild);
    const error = caughtName(() => operation(node));
    const row = [
      error,
      node.ownerDocument === document,
      node.parentNode?.nodeType ?? null,
      holder.childNodes.length,
      document.documentElement === root && root.parentNode === document,
      range.startContainer === holder,
      range.startOffset,
      range.endOffset,
      node.parentNode?.textContent ?? null,
    ];
    node.remove();
    holder.remove();
    return row;
  };
  const selfAncestorRow = () => {
    const target = document.createElement("section");
    target.innerHTML = "<i>retained</i>";
    document.body.append(target);
    const originalParent = target.parentNode;
    const error = caughtName(() => target.append(target, "text"));
    const row = [
      error,
      target.parentNode === originalParent,
      target.parentNode?.nodeType ?? null,
      target.innerHTML,
      target.parentNode?.textContent ?? null,
    ];
    target.remove();
    return row;
  };
  const fragmentOverlapRow = (order) => {
    const target = document.createElement("section");
    document.body.append(target);
    const fragment = source.createDocumentFragment();
    const first = source.createElement("b"),
      last = source.createElement("i");
    first.textContent = "first";
    last.textContent = "last";
    fragment.append(first, last);
    if (order === "child-text-fragment") target.append(first, "mid", fragment);
    else target.append(fragment, first, fragment);
    const row = [
      target.innerHTML,
      fragment.ownerDocument === source,
      fragment.childNodes.length,
      first.ownerDocument === document,
      last.ownerDocument === document,
    ];
    target.remove();
    return row;
  };
  const fragmentFailureRow = (count) => {
    const fragment = source.createDocumentFragment(),
      children = [];
    for (let index = 0; index < count; index++) {
      const node = source.createElement("b");
      node.textContent = String(index);
      fragment.append(node);
      children.push(node);
    }
    const error = caughtName(() => document.append(fragment));
    return [
      error,
      fragment.ownerDocument === source,
      fragment.childNodes.length,
      children.map((node) => [node.ownerDocument === source, node.parentNode === fragment]),
    ];
  };
  try {
    for (const [name, operation] of Object.entries(operations))
      rows[name] = operationRow(operation);
    rows.selfAncestor = selfAncestorRow();
    rows.fragmentOverlap = {};
    for (const order of ["child-text-fragment", "fragment-child-fragment"])
      rows.fragmentOverlap[order] = fragmentOverlapRow(order);
    rows.fragmentFailures = {};
    for (const count of [1, 2]) rows.fragmentFailures[count] = fragmentFailureRow(count);
    return rows;
  } finally {
    frame.remove();
  }
})();
