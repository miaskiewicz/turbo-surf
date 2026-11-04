(async () => {
  document.body.style = "margin:0";
  const frame = document.createElement("iframe");
  frame.style = "display:block;width:100px;height:80px;border:0";
  document.body.append(frame);
  if (!frame.contentDocument) await new Promise((resolve) => (frame.onload = resolve));
  const frames = () =>
    new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(resolve, 50))),
    );
  // Settle the frame's first layout before observing it.
  await frames();
  const win = frame.contentWindow;
  const log = [];
  const narrow = win.matchMedia("(max-width: 150px)");
  narrow.addEventListener("change", (event) =>
    log.push(["change", event.type, event.media, event.matches, narrow.matches, win.innerWidth]),
  );
  narrow.onchange = (event) => log.push(["onchange", event.matches]);
  win.addEventListener("resize", (event) =>
    log.push([
      "resize",
      event.type,
      event.bubbles,
      event.cancelable,
      win.innerWidth,
      win.innerHeight,
      narrow.matches,
    ]),
  );
  const before = [win.innerWidth, narrow.matches];
  frame.style.width = "200px";
  await frames();
  log.push("wide");
  const wide = [win.innerWidth, narrow.matches];
  frame.style.width = "120px";
  await frames();
  log.push("narrow");
  frame.style.height = "60px";
  await frames();
  return { before, wide, after: [win.innerWidth, win.innerHeight, narrow.matches], log };
})();
