// Event waits share listener ownership with on/off; deadlines never fabricate events.
export function waitForEmitterEvent(emitter, event, options = {}, defaultTimeout = 30000) {
  const predicate = typeof options === "function" ? options : (options.predicate ?? (() => true));
  const timeout =
    typeof options === "function" ? defaultTimeout : (options.timeout ?? defaultTimeout);
  return new Promise((resolve, reject) => {
    let timer;
    let settled = false;
    const finish = (error, payload) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      emitter.off(event, onEvent);
      emitter.off("close", onClose);
      if (error) reject(error);
      else resolve(payload);
    };
    const onEvent = async (payload) => {
      try {
        if (await predicate(payload)) finish(null, payload);
      } catch (error) {
        finish(error);
      }
    };
    const onClose = () => {
      if (event !== "close") finish(new Error("Page closed while waiting for event"));
    };
    emitter.on(event, onEvent);
    emitter.on("close", onClose);
    if (timeout !== 0)
      timer = setTimeout(
        () =>
          finish(
            Object.assign(
              new Error(`Timeout ${timeout}ms exceeded while waiting for event ${event}`),
              { name: "TimeoutError" },
            ),
          ),
        timeout,
      );
  });
}
