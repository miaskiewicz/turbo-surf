(async () => {
  // Fixed-pixel scrollers keep every observation independent of installed fonts.
  const out = {};
  // A capped frame wait keeps throttled background windows from stalling the control.
  const frame = () =>
    new Promise((done) => {
      requestAnimationFrame(() => done());
      setTimeout(done, 100);
    });
  const settle = async (count = 3) => {
    for (let i = 0; i < count; i++) await frame();
    await new Promise((done) => setTimeout(done, 30));
  };
  // Chrome156 scroll methods return promises: abrupt conversions become rejections.
  const invoke = (callback) => {
    try {
      const value = callback();
      return {
        value,
        sync: value instanceof Promise ? "promise" : value === undefined ? "undefined" : typeof value,
      };
    } catch (error) {
      return { sync: ["threw", error.constructor.name, error.message] };
    }
  };
  const outcome = async (value) => {
    if (!(value instanceof Promise)) return null;
    try {
      const result = await value;
      return [
        "resolved",
        result === undefined ? "undefined" : result === null ? "null" : String(result),
      ];
    } catch (error) {
      return ["rejected", error.constructor.name, error.message];
    }
  };
  const failure = async (callback) => {
    const call = invoke(callback);
    return [call.sync, await outcome(call.value)];
  };
  const describe = (target, name) => {
    const d = Object.getOwnPropertyDescriptor(target, name);
    if (!d) return null;
    const f = d.value || d.get;
    return {
      kind: "value" in d ? "value" : "accessor",
      writable: d.writable ?? null,
      enumerable: d.enumerable,
      configurable: d.configurable,
      setter: d.set ? [d.set.name, d.set.length] : null,
      name: f.name,
      length: f.length,
    };
  };
  document.body.style.cssText = "margin:0";
  document.body.innerHTML = `
    <div id=spacer style="height:1000px;width:10px"></div>
    <div id=outer style="width:200px;height:150px;overflow:auto">
      <div style="height:300px;width:10px"></div>
      <div id=inner style="width:150px;height:100px;overflow:auto;margin-left:400px">
        <div style="height:250px;width:10px"></div>
        <div id=target style="width:20px;height:30px;margin-left:200px"></div>
        <div style="height:250px;width:10px"></div>
      </div>
      <div style="height:300px;width:10px"></div>
    </div>
    <div id=box style="width:100px;height:80px;overflow:auto"><div style="width:300px;height:400px"></div></div>
    <div id=hidden style="width:100px;height:80px;overflow:hidden"><div style="width:300px;height:400px"></div></div>
    <div id=visible style="width:100px;height:80px;overflow:visible"><div style="width:300px;height:400px"></div></div>
    <div id=clip style="width:100px;height:80px;overflow:clip"><div style="width:300px;height:400px"></div></div>
    <div id=none style="display:none;width:100px;height:80px;overflow:auto"><div style="width:300px;height:400px"></div></div>
    <div id=tall style="height:2000px;width:2500px"></div>`;
  const $ = (id) => document.getElementById(id);
  const [outer, inner, target, box] = ["outer", "inner", "target", "box"].map($);
  const offsets = (element) => [element.scrollLeft, element.scrollTop];
  const reset = () => {
    for (const element of [outer, inner, box]) element.scroll(0, 0);
    window.scroll(0, 0);
  };
  out.viewport = [innerWidth, innerHeight, devicePixelRatio];
  // Scroll event timing, coalescing and trust.
  const events = [];
  const record = (label) => (event) =>
    events.push([
      label,
      event.type,
      event.target === document ? "document" : event.target.id,
      event.eventPhase,
      event.bubbles,
      event.cancelable,
      event.isTrusted,
      event.constructor.name,
    ]);
  for (const [node, label] of [
    [outer, "outer"],
    [inner, "inner"],
    [box, "box"],
    [document, "document"],
    [window, "window"],
  ])
    for (const type of ["scroll", "scrollend"]) node.addEventListener(type, record(label));
  const mark = (label) => events.push([label]);
  const step = async (label, action) => {
    events.length = 0;
    requestAnimationFrame(() => mark("rafBefore"));
    const result = action();
    if (result instanceof Promise) result.then(() => mark("resolved"));
    mark("sync");
    queueMicrotask(() => mark("microtask"));
    setTimeout(() => mark("timeout0"), 0);
    requestAnimationFrame(() => {
      mark("raf1");
      requestAnimationFrame(() => {
        mark("raf2");
        requestAnimationFrame(() => mark("raf3"));
      });
    });
    await settle(4);
    await new Promise((done) => setTimeout(done, 200));
    events.push(["state", document.visibilityState, document.hasFocus()]);
    return [label, events.slice()];
  };
  out.events = [];
  out.events.push(
    await step("coalesced", () => {
      outer.scrollTop = 10;
      outer.scrollTop = 20;
    }),
  );
  out.events.push(await step("method", () => outer.scroll(0, 25)));
  out.events.push(await step("methodUnchanged", () => outer.scroll(0, 25)));
  out.events.push(
    await step("methodPair", () => {
      outer.scroll(0, 26);
      return outer.scroll(0, 27);
    }),
  );
  out.events.push(await step("unchanged", () => {
    outer.scrollTop = 27;
  }));
  out.events.push(
    await step("returned", () => {
      outer.scrollTop = 40;
      outer.scrollTop = 27;
    }),
  );
  out.events.push(await step("clampedNoop", () => outer.scroll(0, -50)));
  out.events.push(await step("viewport", () => window.scrollTo(0, 100)));
  out.events.push(await step("rootElement", () => {
    document.documentElement.scrollTop = 150;
  }));
  out.events.push(
    await step("order", () => {
      box.scrollTop = 5;
      outer.scrollTop = 30;
      box.scrollTop = 6;
    }),
  );
  out.events.push(
    await step("intoView", () => target.scrollIntoView({ block: "center", inline: "center" })),
  );
  out.events.push(
    await step("removed", () => {
      box.scrollTop = 20;
      box.remove();
      return undefined;
    }),
  );
  document.body.append(box);
  out.events.push(await step("scrollBy", () => outer.scrollBy(0, 1)));
  reset();
  await settle();

  return out;
})();

