(async () => {
  switch (globalThis.__timerClockPhase) {
    case "bodyBeforeLaterTimer": {
      const response = await fetch("/body");
      let completed = false;
      response.text().then(() => {
        completed = true;
      });
      return new Promise((resolve) => setTimeout(() => resolve(completed), 100));
    }
    case "windowDeadlineAbort": {
      const controller = new AbortController();
      const response = await fetch("/stall", { signal: controller.signal });
      setTimeout(() => controller.abort(), 30);
      try {
        await response.text();
        return "unexpected success";
      } catch (error) {
        return error.name;
      }
    }
    case "heldNativeTimeout": {
      const signal = AbortSignal.timeout(20);
      return new Promise((resolve) => setTimeout(() => resolve(signal.aborted), 30));
    }
    case "prepareNativeTimeout": {
      globalThis.timeoutDelivered = false;
      (() => {
        const signal = AbortSignal.timeout(20);
        signal.addEventListener("abort", () => {
          timeoutDelivered = signal.aborted && signal.reason.name === "TimeoutError";
        });
      })();
      return "prepared";
    }
    case "retainedNativeTimeout":
      return new Promise((resolve) => setTimeout(() => resolve(timeoutDelivered), 30));
    default:
      throw new Error("Unknown timer clock phase");
  }
})();
