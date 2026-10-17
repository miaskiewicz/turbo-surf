(async () => {
  const order = [];
  const node = document.createElement("div");
  document.body.append(node);
  const observer = new MutationObserver(() => order.push("observer"));
  observer.observe(node, { attributes: true });
  Promise.resolve().then(() => order.push("promise-first"));
  queueMicrotask(function () {
    "use strict";
    order.push(this === undefined ? "microtask-undefined-this" : "microtask-other-this");
    Promise.resolve().then(() => order.push("nested-promise"));
  });
  node.setAttribute("data-test", "1");
  Promise.resolve().then(() => order.push("promise-last"));
  setTimeout(() => order.push("timer"), 0);
  const invalid = [undefined, null, 0, {}, "code"].map((value) => {
    try {
      queueMicrotask(value);
      return "accepted";
    } catch (error) {
      return error.name;
    }
  });
  let borrowed;
  try {
    queueMicrotask.call({}, () => {});
    borrowed = "accepted";
  } catch (error) {
    borrowed = error.name;
  }
  const callback = new Proxy(() => order.push("proxy-callback"), {});
  const result = queueMicrotask(callback);
  await new Promise((resolve) => setTimeout(resolve, 0));
  observer.disconnect();
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "queueMicrotask");
  return {
    order,
    invalid,
    borrowed,
    result: typeof result,
    descriptor: {
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
      writable: descriptor.writable,
      name: queueMicrotask.name,
      length: queueMicrotask.length,
      native: Function.prototype.toString.call(queueMicrotask).includes("[native code]"),
    },
  };
})();
