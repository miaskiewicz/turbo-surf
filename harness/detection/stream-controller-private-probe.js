(() => {
  const original = Array.prototype.includes,
    apply = Reflect.apply;
  const desired = Object.getOwnPropertyDescriptor(
    ReadableStreamDefaultController.prototype,
    "desiredSize",
  ).get;
  let rawExposed = false,
    callback = false,
    enqueueWorks = false;
  try {
    Array.prototype.includes = function (value) {
      if (this.length === 3 && this[0] === "cancel" && this[1] === "abort" && this[2] === "close")
        return true;
      return apply(original, this, [value]);
    };
    new ReadableStream({
      start(controller) {
        callback = true;
        try {
          apply(desired, controller, []);
        } catch {
          rawExposed = true;
        }
        controller.enqueue("safe");
        controller.close();
        enqueueWorks = true;
      },
    });
  } finally {
    Array.prototype.includes = original;
  }
  return { rawExposed, callback, enqueueWorks };
})();
