// Private stream transport over the same native ports used by structured clone.
(function (api) {
  "use strict";
  const apply = Reflect.apply,
    then = Promise.prototype.then,
    define = Object.defineProperty;
  const index = (array, key, value) =>
    define(array, key, {
      __proto__: null,
      value,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  const PromiseType = Promise,
    resolve = Promise.resolve.bind(Promise);
  const Readable = api.Readable,
    Writable = api.Writable,
    TypeErrorType = TypeError;
  const readable = Object.getOwnPropertyDescriptors(Readable.prototype);
  const writable = Object.getOwnPropertyDescriptors(Writable.prototype);
  const rc = Object.getOwnPropertyDescriptors(api.ReadableController.prototype);
  const wc = Object.getOwnPropertyDescriptors(api.WritableController.prototype);
  const handled = (promise) => {
    apply(then, promise, [undefined, () => undefined]);
  };
  const close = (port) => api.close(port);
  const send = (port, type, value) => api.post(port, { type, value });
  const sendError = (port, error) => {
    try {
      send(port, "error", error);
    } catch {
      /* Clone failure already errors the peer. */
    }
  };
  const pack = (port, type, value) => {
    try {
      send(port, type, value);
    } catch (error) {
      sendError(port, error);
      throw error;
    }
  };
  function incoming(port) {
    let controller;
    const result = new Readable(
      {
        start(value) {
          controller = value;
          api.listen(
            port,
            (message) => {
              try {
                if (message.type === "chunk") apply(rc.enqueue.value, controller, [message.value]);
                else if (message.type === "close") {
                  apply(rc.close.value, controller, []);
                  close(port);
                } else if (message.type === "error") {
                  apply(rc.error.value, controller, [message.value]);
                  close(port);
                }
              } catch (error) {
                sendError(port, error);
                close(port);
              }
            },
            (error) => {
              sendError(port, error);
              apply(rc.error.value, controller, [error]);
              close(port);
            },
          );
        },
        pull() {
          send(port, "pull", undefined);
        },
        cancel(reason) {
          try {
            send(port, "error", reason);
          } catch (error) {
            const contextual = api.cancelFailure ? api.cancelFailure(error) : error;
            sendError(port, contextual);
            throw contextual;
          } finally {
            close(port);
          }
        },
      },
      { highWaterMark: 0 },
    );
    return result;
  }
  function outgoing(port, active) {
    let controller, pending;
    const deferred = () => {
      let resolve;
      const promise = new PromiseType((r) => {
        resolve = r;
      });
      return { promise, resolve };
    };
    pending = deferred();
    return new Writable(
      {
        start(value) {
          controller = value;
          api.listen(
            port,
            (message) => {
              if (message.type === "pull") {
                if (pending) {
                  pending.resolve();
                  pending = undefined;
                }
              } else if (message.type === "error") {
                apply(wc.error.value, controller, [message.value]);
                if (pending) {
                  pending.resolve();
                  pending = undefined;
                }
                close(port);
              }
            },
            (error) => {
              sendError(port, error);
              apply(wc.error.value, controller, [error]);
              if (pending) {
                pending.resolve();
                pending = undefined;
              }
              close(port);
            },
          );
        },
        write(chunk) {
          if (active && !active()) return new PromiseType(() => {});
          if (!pending) pending = { promise: resolve(undefined), resolve() {} };
          return apply(then, pending.promise, [
            () => {
              pending = deferred();
              try {
                pack(port, "chunk", chunk);
              } catch (error) {
                close(port);
                throw error;
              }
            },
          ]);
        },
        close() {
          send(port, "close", undefined);
          close(port);
        },
        abort(reason) {
          try {
            pack(port, "error", reason);
          } finally {
            close(port);
          }
        },
      },
      { highWaterMark: 1 },
    );
  }
  const lock = (value, kind) =>
    apply(kind === 1 ? readable.locked.get : writable.locked.get, value, []);
  const pipe = (source, sink) => handled(apply(readable.pipeTo.value, source, [sink]));
  return {
    validate(front) {
      const state = api.state(front);
      if (!state) return 0;
      if (state.kind === 3) {
        if (lock(state.readable, 1) || lock(state.writable, 2)) return -3;
      } else if (lock(state.raw, state.kind)) return -state.kind;
      return state.kind;
    },
    prepare(front) {
      const state = api.state(front),
        source = [],
        receiver = [];
      for (let i = 0; i < (state.kind === 3 ? 2 : 1); i++) {
        const pair = api.channel();
        index(source, i, pair[0]);
        index(receiver, i, pair[1]);
      }
      return { state, source, receiver };
    },
    commit(plan, plans) {
      const state = plan.state;
      const connect = (raw, kind, port) => {
        if (lock(raw, kind)) throw new TypeErrorType("Stream became locked during transfer");
        if (kind === 1) pipe(raw, outgoing(port, state.active));
        else pipe(incoming(port), raw);
      };
      if (state.kind !== 3) connect(state.raw, state.kind, plan.source[0]);
      else {
        connect(state.readable, 1, plan.source[0]);
        for (let i = 0; i < plans.length; i++) {
          if (
            plans[i] !== plan &&
            plans[i].state.kind === 1 &&
            plans[i].state.raw === state.readable
          )
            throw new TypeErrorType("Readable stream occurs twice in expanded transfer list");
        }
        connect(state.writable, 2, plan.source[1]);
      }
    },
    abandon(plan) {
      for (let i = 0; i < plan.source.length; i++) {
        close(plan.source[i]);
        close(plan.receiver[i]);
      }
    },
    restore(kind, first, second) {
      if (kind === 1) return api.wrap(incoming(first), "ReadableStream");
      if (kind === 2) return api.wrap(outgoing(first), "WritableStream");
      return api.transform(incoming(first), outgoing(second));
    },
  };
});
