// Shared real-browser/native localhost lifecycle collector.
(async () => {
  const result = {};
  const fixtures = [];
  const create = (mode = "auto", tag = "div") => {
    const node = document.createElement(tag);
    node.popover = mode;
    document.body.append(node);
    fixtures.push(node);
    return node;
  };
  const state = (node) => node.matches(":popover-open");
  const safe = (run) => {
    try {
      const value = run();
      return value === undefined ? { undefined: true } : value;
    } catch (error) {
      return { error: error.name };
    }
  };
  const listen = (node, rows) => {
    for (const type of ["beforetoggle", "toggle"])
      node.addEventListener(type, (event) =>
        rows.push([
          type,
          event.oldState,
          event.newState,
          event.cancelable,
          event.isTrusted,
          event instanceof ToggleEvent,
          event.source?.id ?? null,
          state(node),
        ]),
      );
  };
  const drain = () => new Promise((done) => setTimeout(done, 0));
  const plain = create("manual"),
    rows = [];
  listen(plain, rows);
  result.basic = [
    safe(() => plain.showPopover()),
    state(plain),
    safe(() => plain.showPopover()),
    safe(() => plain.togglePopover(true)),
    safe(() => plain.hidePopover()),
    state(plain),
    safe(() => plain.hidePopover()),
    safe(() => plain.togglePopover()),
    safe(() => plain.togglePopover()),
  ];
  await drain();
  result.coalescedEvents = rows.slice();
  const canceled = create("manual");
  canceled.addEventListener("beforetoggle", (event) => {
    if (event.newState === "open") event.preventDefault();
  });
  result.cancelOpen = [
    safe(() => canceled.showPopover()),
    state(canceled),
    safe(() => canceled.togglePopover()),
    state(canceled),
  ];
  const auto = [create(), create()],
    autoRows = [];
  auto.forEach((node, index) => {
    node.id = `auto-${index}`;
    for (const type of ["beforetoggle", "toggle"])
      node.addEventListener(type, (event) =>
        autoRows.push([
          index,
          type,
          event.oldState,
          event.newState,
          event.cancelable,
          event.isTrusted,
        ]),
      );
  });
  auto[0].showPopover();
  auto[1].showPopover();
  result.autoClose = auto.map(state);
  await drain();
  result.autoEvents = autoRows.slice();
  const nested = create(),
    nestedChild = create();
  nested.append(nestedChild);
  nested.showPopover();
  nestedChild.showPopover();
  result.nested = [state(nested), state(nestedChild), state(auto[1])];
  const hint = create("hint"),
    secondHint = create("hint");
  hint.showPopover();
  secondHint.showPopover();
  result.hints = [state(nested), state(nestedChild), state(hint), state(secondHint)];
  const unrelated = create();
  unrelated.showPopover();
  result.newAuto = [state(nested), state(nestedChild), state(secondHint), state(unrelated)];
  const detached = create("manual"),
    detachedRows = [];
  listen(detached, detachedRows);
  detached.showPopover();
  await drain();
  detachedRows.length = 0;
  detached.remove();
  result.removed = [
    state(detached),
    safe(() => detached.hidePopover()),
    safe(() => detached.showPopover()),
  ];
  await drain();
  result.removalEvents = detachedRows;
  const attr = create("manual"),
    attrRows = [];
  listen(attr, attrRows);
  attr.showPopover();
  await drain();
  attrRows.length = 0;
  attr.removeAttribute("popover");
  result.attributeRemoved = [state(attr), safe(() => attr.hidePopover())];
  await drain();
  result.attributeEvents = attrRows;
  const reentrant = create("manual");
  reentrant.addEventListener("beforetoggle", () => reentrant.removeAttribute("popover"), {
    once: true,
  });
  result.attributeDuringBefore = [safe(() => reentrant.showPopover()), state(reentrant)];
  const disconnect = create("manual");
  disconnect.addEventListener("beforetoggle", () => disconnect.remove(), { once: true });
  result.disconnectDuringBefore = [safe(() => disconnect.showPopover()), state(disconnect)];
  const disabled = create("manual", "button");
  disabled.disabled = true;
  result.disabled = [safe(() => disabled.showPopover()), state(disabled)];
  const source = document.createElement("button");
  source.id = "source";
  document.body.append(source);
  fixtures.push(source);
  const sourced = create("manual"),
    sourceRows = [];
  listen(sourced, sourceRows);
  result.source = [
    safe(() => sourced.showPopover({ source })),
    safe(() => sourced.hidePopover({ source })),
  ];
  await drain();
  result.sourceEvents = sourceRows;
  const option = create("manual");
  result.options = [
    safe(() => option.togglePopover({ force: true, source })),
    state(option),
    safe(() => option.togglePopover({ force: false, source })),
    state(option),
    safe(() => option.showPopover({ source: {} })),
  ];
  const selector = create("manual");
  selector.id = "probe-open";
  selector.showPopover();
  result.selectors = [
    selector.matches("div:popover-open"),
    document.querySelector("#probe-open:popover-open") === selector,
    document.querySelectorAll(":not(:popover-open)#probe-open").length,
    document.body.matches(":has(> #probe-open:popover-open)"),
    selector.matches("[title=':popover-open']"),
    safe(() => selector.matches(":popover-open(")),
  ];
  result.moves = {};
  for (const [name, move] of Object.entries({
    insertSelf: (node) => node.parentNode.insertBefore(node, node),
    replaceSelf: (node) => node.parentNode.replaceChild(node, node),
    replaceWithSelf: (node) => node.replaceWith(node),
    appendExisting: (node) => node.parentNode.appendChild(node),
    moveBefore: (node) => node.parentNode.moveBefore(node, null),
  })) {
    const node = create("manual"),
      events = [];
    listen(node, events);
    node.showPopover();
    await drain();
    events.length = 0;
    const outcome = safe(() => move(node));
    await drain();
    result.moves[name] = [outcome, state(node), events];
  }
  result.conversion = {};
  for (const [name, value] of Object.entries({
    number: 42,
    string: "x",
    boolean: true,
    nullValue: null,
  })) {
    const node = create("manual");
    result.conversion[name] = [safe(() => node.showPopover(value)), state(node)];
  }
  const sourceReads = create("manual");
  let reads = 0;
  sourceReads.showPopover({
    get source() {
      reads++;
      return source;
    },
  });
  result.sourceReads = reads;
  const modeChanged = create("auto");
  modeChanged.addEventListener(
    "beforetoggle",
    () => {
      modeChanged.popover = "manual";
    },
    { once: true },
  );
  result.modeChangedBefore = [safe(() => modeChanged.showPopover()), state(modeChanged)];
  const removedPending = create("manual"),
    pendingRows = [];
  listen(removedPending, pendingRows);
  removedPending.showPopover();
  removedPending.remove();
  await drain();
  result.pendingRemoval = pendingRows;
  const namespaced = document.createElement("div");
  namespaced.setAttributeNS("urn:foreign", "popover", "auto");
  document.body.append(namespaced);
  fixtures.push(namespaced);
  result.namespaced = [
    namespaced.popover,
    namespaced.getAttribute("popover"),
    safe(() => namespaced.showPopover()),
    state(namespaced),
  ];
  result.modeMatrix = {};
  for (const outer of ["auto", "hint", "manual"])
    for (const inner of ["auto", "hint", "manual"]) {
      const parent = create(outer),
        child = create(inner);
      parent.append(child);
      parent.showPopover();
      child.showPopover();
      result.modeMatrix[`${outer}/${inner}`] = [state(parent), state(child)];
      parent.remove();
      await drain();
    }
  for (const node of fixtures) node.remove();
  await drain();
  const encoded = JSON.stringify(result);
  globalThis.__RESULT = encoded;
  return encoded;
})();
