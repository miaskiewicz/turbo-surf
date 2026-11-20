// Public MessageChannel tasks and Promise reactions, with exact results alongside timing.
export async function measurePromiseWork(page, options) {
  const { depth: requestedDepth, fanout: requestedFanout, turns: requestedTurns } = options;
  for (const value of [requestedDepth, requestedFanout, requestedTurns])
    if (!Number.isSafeInteger(value) || value < 0)
      throw new TypeError("non-negative integer workload required");
  if (!requestedTurns) throw new RangeError("at least one MessageChannel task required");
  const started = performance.now();
  const result = await page.evaluate(async ({ depth, fanout, turns }) => {
    const parent = Promise.resolve(3);
    const channel = new MessageChannel();
    let active = false;
    let completed = 0;
    let checksum = 0;
    await new Promise((resolve, reject) => {
      const close = () => {
        channel.port1.close();
        channel.port2.close();
      };
      channel.port1.addEventListener("message", async (event) => {
        try {
          if (active || event.data !== completed)
            throw new Error("MessageChannel task reentered before reactions completed");
          active = true;
          if (completed + 1 < turns) channel.port2.postMessage(completed + 1);
          let chain = parent;
          for (let index = 0; index < depth; index++) chain = chain.then((value) => value + 1);
          const siblings = Array.from({ length: fanout }, (_, index) =>
            parent.then((value) => value + index),
          );
          const values = await Promise.all([chain, ...siblings]);
          checksum += values.reduce((sum, value) => sum + value, 0);
          completed++;
          active = false;
          if (completed === turns) {
            close();
            resolve();
          }
        } catch (error) {
          active = false;
          close();
          reject(error);
        }
      });
      channel.port1.start();
      channel.port2.postMessage(0);
    });
    return { completed, checksum, depth, fanout, turns };
  }, options);
  return { ...result, elapsedMs: performance.now() - started };
}

export function expectedPromiseWork({ depth, fanout, turns }) {
  return {
    completed: turns,
    checksum: turns * (3 + depth + 3 * fanout + (fanout * (fanout - 1)) / 2),
  };
}
