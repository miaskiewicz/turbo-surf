// Authored control: page hooks cannot observe private event listener records.
(() => {
  const define = Object.defineProperty,
    descriptor = Object.getOwnPropertyDescriptor,
    iterator = Symbol.iterator,
    species = Symbol.species;
  const result = {};
  function scenario(name, install, restore) {
    let hooks = 0,
      calls = 0,
      aborts = 0,
      error = null;
    const poison = () => {
      hooks++;
      throw new Error("Authored private event hook");
    };
    const target = new EventTarget(),
      controller = new AbortController(),
      event = new Event("authored");
    const audio = new OfflineAudioContext(1, 32, 44100);
    const listener = () => calls++;
    try {
      install(poison);
      controller.signal.addEventListener("abort", () => aborts++, { once: true });
      target.addEventListener("authored", listener, { signal: controller.signal });
      target.dispatchEvent(event);
      target.removeEventListener("authored", listener);
      target.addEventListener("authored", listener, { once: true });
      target.dispatchEvent(event);
      target.dispatchEvent(event);
      window.addEventListener("authored", listener, { signal: controller.signal });
      window.dispatchEvent(event);
      audio.addEventListener("authored", listener, { once: true });
      audio.dispatchEvent(event);
      audio.dispatchEvent(event);
      // Native object construction must also bypass inherited state setters.
      const oscillator = audio.createOscillator();
      oscillator.addEventListener("authored", listener, { once: true });
      oscillator.dispatchEvent(event);
      controller.abort("authored reason");
      window.dispatchEvent(event);
    } catch (caught) {
      error = caught.name;
    } finally {
      restore();
      window.removeEventListener("authored", listener);
    }
    result[name] = {
      hooks,
      calls,
      aborts,
      error,
      reason: controller.signal.reason,
      targetKeys: Reflect.ownKeys(target),
      signalKeys: Reflect.ownKeys(controller.signal),
      audioKeys: Reflect.ownKeys(audio),
    };
  }
  const zero = descriptor(Array.prototype, "0");
  scenario(
    "indexSetter",
    (poison) => define(Array.prototype, "0", { configurable: true, set: poison }),
    () => {
      if (zero) define(Array.prototype, "0", zero);
      else delete Array.prototype[0];
    },
  );
  const constructor = descriptor(Array.prototype, "constructor");
  scenario(
    "species",
    (poison) => {
      const replacement = Object.create(null);
      define(replacement, species, { get: poison });
      define(Array.prototype, "constructor", { ...constructor, value: replacement });
    },
    () => define(Array.prototype, "constructor", constructor),
  );
  const arrayIterator = descriptor(Array.prototype, iterator);
  scenario(
    "iterator",
    (poison) => define(Array.prototype, iterator, { ...arrayIterator, value: poison }),
    () => define(Array.prototype, iterator, arrayIterator),
  );
  const listeners = descriptor(Object.prototype, "listeners"),
    handlers = descriptor(Object.prototype, "handlers");
  scenario(
    "stateSetters",
    (poison) => {
      define(Object.prototype, "listeners", { configurable: true, set: poison });
      define(Object.prototype, "handlers", { configurable: true, set: poison });
    },
    () => {
      if (listeners) define(Object.prototype, "listeners", listeners);
      else delete Object.prototype.listeners;
      if (handlers) define(Object.prototype, "handlers", handlers);
      else delete Object.prototype.handlers;
    },
  );
  return result;
})();
