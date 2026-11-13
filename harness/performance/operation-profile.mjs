import { AsyncLocalStorage } from "node:async_hooks";
import { types } from "node:util";

// Diagnostic timings are inclusive: nested operations must not be added together.
export function createOperationProfile(now = () => performance.now()) {
  const context = new AsyncLocalStorage();
  const operations = new Map();
  const restorers = [];
  function wrap(prototype, names, prefix) {
    for (const name of names) {
      const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
      if (typeof descriptor?.value !== "function") continue;
      const original = descriptor.value;
      const label = `${prefix}.${name}`;
      const wrapped = function (...args) {
        const root = !context.getStore();
        const start = now();
        const entry = operations.get(label) ?? {
          calls: 0,
          completed: 0,
          errors: 0,
          rootCalls: 0,
          elapsedMs: 0,
          rootElapsedMs: 0,
          maxMs: 0,
        };
        operations.set(label, entry);
        entry.calls++;
        if (root) entry.rootCalls++;
        const finish = (failed) => {
          const elapsed = Math.max(0, now() - start);
          entry.completed++;
          if (failed) entry.errors++;
          entry.elapsedMs += elapsed;
          if (root) entry.rootElapsedMs += elapsed;
          entry.maxMs = Math.max(entry.maxMs, elapsed);
        };
        return context.run(true, () => {
          try {
            const result = Reflect.apply(original, this, args);
            if (types.isPromise(result))
              result.then(
                () => finish(false),
                () => finish(true),
              );
            else finish(false);
            return result;
          } catch (error) {
            finish(true);
            throw error;
          }
        });
      };
      Object.defineProperty(prototype, name, { ...descriptor, value: wrapped });
      restorers.push(() => Object.defineProperty(prototype, name, descriptor));
    }
  }
  return {
    wrap,
    snapshot: () => Object.fromEntries([...operations].sort(([a], [b]) => a.localeCompare(b))),
    restore() {
      for (const restore of restorers.reverse()) restore();
      restorers.length = 0;
    },
  };
}
