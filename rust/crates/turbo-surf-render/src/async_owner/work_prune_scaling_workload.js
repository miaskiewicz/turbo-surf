(async () => {
  const size = __SIZE__;
  const fanout = __FANOUT__;
  const parent = Promise.resolve(3);
  const channel = new MessageChannel();
  let active = false;
  let completed = 0;
  let checksum = 0;
  await new Promise((resolve, reject) => {
    channel.port1.addEventListener("message", async (event) => {
      try {
        if (active || event.data !== completed) throw new Error("task reentered");
        active = true;
        if (completed === 0) channel.port2.postMessage(1);
        if (fanout) {
          const values = await Promise.all(
            Array.from({ length: size }, (_, index) => parent.then((value) => value + index)),
          );
          checksum += values.reduce((sum, value) => sum + value, 0);
        } else {
          let chain = parent;
          for (let index = 0; index < size; index++) chain = chain.then((value) => value + 1);
          checksum += await chain;
        }
        completed++;
        active = false;
        if (completed === 2) {
          channel.port1.close();
          channel.port2.close();
          resolve();
        }
      } catch (error) {
        channel.port1.close();
        channel.port2.close();
        reject(error);
      }
    });
    channel.port1.start();
    channel.port2.postMessage(0);
  });
  return { completed, checksum, healthy: document.querySelector("h1").textContent };
})();
