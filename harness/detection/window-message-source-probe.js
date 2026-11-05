(async () => {
  // postMessage event.source per posting path (turbo-surf#85/#90): the source is the
  // incumbent realm's WindowProxy, so each row compares e.source with the sender's
  // window as the receiver sees it. Plus `new Window()` / `Window()` (turbo-surf#129).
  const tick = () => new Promise((done) => setTimeout(done, 40));
  const frame = (parent) => {
    const element = parent.document.createElement("iframe");
    parent.document.body.append(element);
    return element.contentWindow;
  };
  const rows = [];
  const a = frame(window);
  const b = frame(window);
  const g = frame(a);
  window.onmessage = (e) => {
    const sender = { a, b, g, top: window }[e.data.split(":")[0]];
    rows.push(`top<-${e.data}:${e.source === sender}`);
  };
  a.eval(`onmessage=e=>parent.postMessage('a:seen '+e.data+':'+(e.source===parent),'*')`);
  b.eval(
    `onmessage=e=>parent.postMessage('b:seen '+e.data+':'+(e.source===parent.frames[0])+','+(e.source===parent),'*')`,
  );
  a.eval("parent.postMessage('a:eval','*')");
  await tick();
  a.eval("setTimeout(()=>parent.postMessage('a:timer','*'),0)");
  await tick();
  const script = a.document.createElement("script");
  script.textContent = "parent.postMessage('a:script','*')";
  a.document.body.append(script);
  await tick();
  a.eval("()=>parent.postMessage('a:function','*')")();
  await tick();
  g.eval("top.postMessage('g:nested','*')");
  await tick();
  a.parent.postMessage("top:via-child-proxy", "*");
  await tick();
  window.postMessage("top:self", "*");
  await tick();
  a.postMessage("down", "*");
  await tick();
  a.eval("parent.frames[1].postMessage('sibling','*')");
  await tick();
  b.postMessage("from-top", "*");
  await tick();
  const errors = [];
  for (const run of [() => new Window(), () => Window()]) {
    try {
      run();
      errors.push("no throw");
    } catch (error) {
      errors.push(`${error.constructor === TypeError}:${error.message}`);
    }
  }
  return { rows, errors };
})();
