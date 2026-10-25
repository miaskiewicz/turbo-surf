// Observable, Subscriber and EventTarget.prototype.when as Chrome 156 ships them
// (Blink observable.cc, subscriber.cc, event_target.cc): one ref-counted producer
// per Observable, consumer aborts as abort algorithms that run before any abort
// listener, and the binding context Blink stamps on the errors it raises itself.
{
  const realm = globalThis,
    make = __G.makeNativeFunction;
  const apply = Reflect.apply,
    define = Object.defineProperty,
    create = Object.create,
    descriptor = Object.getOwnPropertyDescriptor;
  const TypeErrorType = TypeError,
    RangeErrorType = RangeError,
    PromiseType = Promise,
    StringType = String;
  const then = Promise.prototype.then,
    asUintN = BigInt.asUintN,
    BigIntType = BigInt,
    trunc = Math.trunc;
  const iteratorSymbol = Symbol.iterator,
    asyncIteratorSymbol = Symbol.asyncIterator,
    toStringTag = Symbol.toStringTag;
  const Controller = AbortController,
    abortController = AbortController.prototype.abort,
    controllerSignal = descriptor(AbortController.prototype, "signal").get,
    signalAborted = descriptor(AbortSignal.prototype, "aborted").get,
    signalReason = descriptor(AbortSignal.prototype, "reason").get;
  const listen = EventTarget.prototype.addEventListener,
    unlisten = EventTarget.prototype.removeEventListener,
    trustedGetter = descriptor(Event.prototype, "isTrusted")?.get,
    typeGetter = descriptor(Event.prototype, "type")?.get;
  const abortAlgorithm = __web.abortAlgorithm,
    anySignal = __web.anySignal;
  const observables = __interfaceMap("Observable"),
    subscribers = __interfaceMap("Subscriber");
  const report = (error) => ops.op_report_exception(error);
  const isObject = (value) =>
    value !== null && (typeof value === "object" || typeof value === "function");

  // Growable records never touch page-mutable Array.prototype accessors or methods.
  class List {
    constructor() {
      this.length = 0;
    }
    add(value) {
      define(this, this.length, {
        __proto__: null,
        value,
        writable: true,
        enumerable: true,
        configurable: true,
      });
      this.length++;
    }
    pop() {
      const value = this[--this.length];
      delete this[this.length];
      return value;
    }
    shift() {
      const value = this[0];
      for (let index = 1; index < this.length; index++) this[index - 1] = this[index];
      delete this[--this.length];
      return value;
    }
    remove(value) {
      let found = -1;
      for (let index = 0; index < this.length; index++) if (this[index] === value) found = index;
      if (found < 0) return;
      for (let index = found + 1; index < this.length; index++) this[index - 1] = this[index];
      delete this[--this.length];
    }
    copy() {
      const result = new List();
      for (let index = 0; index < this.length; index++) result.add(this[index]);
      return result;
    }
  }

  // The innermost Blink binding frame. An exception Blink throws from C++ takes
  // that frame's "Failed to execute" context as it is thrown, so the same failure
  // reads differently under subscribe(), Subscriber#next() or dispatchEvent().
  class Frame {
    constructor(name, iface) {
      this.name = name;
      this.iface = iface;
    }
  }
  const SCRIPT = new Frame(null, null),
    EMPTY = new Frame(null, null),
    DISPATCH = new Frame("dispatchEvent", "EventTarget"),
    ABORT = new Frame("abort", "AbortController");
  const frames = new List();
  const enter = (frame) => frames.add(frame);
  const leave = () => frames.pop();
  const top = () => (frames.length ? frames[frames.length - 1] : EMPTY);
  const prefix = (name, iface) => "Failed to execute '" + name + "' on '" + iface + "': ";
  const raiseIn = (frame, message, Type = TypeErrorType) =>
    new Type(frame.name === null ? message : prefix(frame.name, frame.iface) + message);
  const raise = (message, Type) => raiseIn(top(), message, Type);
  // Blink's ApplyContextToException rewrites any thrown object's `message`.
  const applyContext = (exception, name) => {
    if (!isObject(exception)) return;
    try {
      const message = prefix(name, "Observable") + StringType(exception.message);
      define(exception, "message", {
        __proto__: null,
        value: message,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    } catch {}
  };

  class Completion {
    constructor(ok, value) {
      this.ok = ok;
      this.value = value;
    }
  }
  // Page script runs under a script frame: a binding it calls is the new innermost.
  const invoke = (callback, thisArg, args) => {
    enter(SCRIPT);
    try {
      return new Completion(true, apply(callback, thisArg, args));
    } catch (error) {
      return new Completion(false, error);
    } finally {
      leave();
    }
  };
  const invokeReport = (callback, args) => {
    const outcome = invoke(callback, undefined, args);
    if (!outcome.ok) report(outcome.value);
  };
  const scripted = (callback, thisArg, args) => {
    enter(SCRIPT);
    try {
      return apply(callback, thisArg, args);
    } finally {
      leave();
    }
  };
  const read = (object, key) => {
    enter(SCRIPT);
    try {
      return object[key];
    } finally {
      leave();
    }
  };

  const aborted = (signal) => apply(signalAborted, signal, []);
  const reasonOf = (signal) => apply(signalReason, signal, []);
  const isSignal = (value) => {
    try {
      apply(signalAborted, value, []);
      return true;
    } catch {
      return false;
    }
  };
  const abort = (controller, hasReason, reason) =>
    apply(abortController, controller, hasReason ? [reason] : []);

  class SubscriberState {
    constructor() {
      this.active = true;
      this.controller = new Controller();
      this.signal = apply(controllerSignal, this.controller, []);
      this.observers = new List();
      this.teardowns = new List();
      this.object = null;
    }
  }
  class ObservableState {
    constructor(callback, delegate) {
      this.callback = callback;
      this.delegate = delegate;
      this.subscriber = null;
    }
  }
  class InternalObserver {
    constructor() {
      this.release = null;
    }
    next() {}
    error(error) {
      report(error);
    }
    complete() {}
  }
  class ScriptObserver extends InternalObserver {
    constructor(next, error, complete) {
      super();
      this.onNext = next;
      this.onError = error;
      this.onComplete = complete;
    }
    next(value) {
      if (this.onNext !== null) invokeReport(this.onNext, [value]);
    }
    error(error) {
      if (this.onError !== null) invokeReport(this.onError, [error]);
      else report(error);
    }
    complete() {
      if (this.onComplete !== null) invokeReport(this.onComplete, []);
    }
  }

  const closeSubscription = (state, hasReason, reason) => {
    if (!state.active) return;
    for (let index = 0; index < state.observers.length; index++) {
      const observer = state.observers[index];
      if (observer.release !== null) {
        observer.release();
        observer.release = null;
      }
    }
    state.active = false;
    // Upstream subscriptions close in the signal's abort algorithms, before its
    // abort event; teardowns then run last-in first-out.
    abort(state.controller, hasReason, reason);
    const teardowns = state.teardowns;
    state.teardowns = new List();
    for (let index = teardowns.length - 1; index >= 0; index--) invokeReport(teardowns[index], []);
  };
  const subscriberNext = (state, value) => {
    if (!state.active) return;
    const observers = state.observers.copy();
    for (let index = 0; index < observers.length; index++) observers[index].next(value);
  };
  const subscriberError = (state, error) => {
    if (!state.active) {
      report(error);
      return;
    }
    closeSubscription(state, true, error);
    const observers = state.observers.copy();
    for (let index = 0; index < observers.length; index++) observers[index].error(error);
  };
  const subscriberComplete = (state) => {
    if (!state.active) return;
    closeSubscription(state, false);
    const observers = state.observers.copy();
    for (let index = 0; index < observers.length; index++) observers[index].complete();
  };
  const addTeardown = (state, teardown) => {
    if (state.active) state.teardowns.add(teardown);
    else invokeReport(teardown, []);
  };
  const consumerUnsubscribe = (state, observer, reason) => {
    if (!state.active) return;
    observer.release = null;
    state.observers.remove(observer);
    if (!state.observers.length) closeSubscription(state, true, reason);
  };
  const attach = (state, observer, signal) => {
    state.observers.add(observer);
    if (signal === null) return true;
    if (aborted(signal)) {
      state.observers.pop();
      return false;
    }
    observer.release = abortAlgorithm(signal, () =>
      consumerUnsubscribe(state, observer, reasonOf(signal)),
    );
    return true;
  };

  let SubscriberPrototype, ObservablePrototype;
  const newObservable = (delegate) => {
    const object = create(ObservablePrototype);
    observables.set(object, new ObservableState(null, delegate));
    return object;
  };
  // A second subscriber joins the live producer; only the first runs its callback.
  const subscribeTo = (observable, observer, signal) => {
    const record = observables.get(observable);
    const current = record.subscriber;
    if (current !== null && current.active) {
      attach(current, observer, signal);
      return;
    }
    const state = new SubscriberState();
    const subscriber = create(SubscriberPrototype);
    subscribers.set(subscriber, state);
    state.object = subscriber;
    record.subscriber = state;
    if (!attach(state, observer, signal)) closeSubscription(state, true, reasonOf(signal));
    if (record.delegate !== null) {
      record.delegate(state);
      return;
    }
    const outcome = invoke(record.callback, undefined, [subscriber]);
    if (outcome.ok) return;
    if (state.active) subscriberError(state, outcome.value);
    else report(outcome.value);
  };

  // Web IDL argument conversion, raising in the calling operation's context.
  const required = (args, count) => {
    if (args.length < count)
      throw raise(
        count +
          " argument" +
          (count > 1 ? "s" : "") +
          " required, but only " +
          args.length +
          " present.",
      );
  };
  const callback = (value, position) => {
    if (typeof value !== "function")
      throw raise("parameter " + position + " is not of type 'Function'.");
    return value;
  };
  const member = (dictionary, key, name) => {
    const value = read(dictionary, key);
    if (value === undefined) return null;
    if (typeof value !== "function")
      throw raise(
        "Failed to read the '" +
          key +
          "' property from '" +
          name +
          "': The given value is not a function.",
      );
    return value;
  };
  const toObserver = (value) => {
    if (typeof value === "function") return new ScriptObserver(value, null, null);
    if (value === undefined || value === null) return new ScriptObserver(null, null, null);
    if (typeof value !== "object")
      throw raise("The provided value is not of type '(Observer or ObserverCallback)'.");
    const complete = member(value, "complete", "Observer"),
      error = member(value, "error", "Observer"),
      next = member(value, "next", "Observer");
    return new ScriptObserver(next, error, complete);
  };
  const toSignal = (value) => {
    if (value === undefined || value === null) return null;
    if (!isObject(value)) throw raise("The provided value is not of type 'SubscribeOptions'.");
    const signal = read(value, "signal");
    if (signal === undefined) return null;
    if (!isSignal(signal))
      throw raise(
        "Failed to read the 'signal' property from 'SubscribeOptions': Failed to convert value to 'AbortSignal'.",
      );
    return signal;
  };
  const toUint64 = (value) => {
    if (typeof value === "symbol") throw raise("Cannot convert a Symbol value to a number");
    if (typeof value === "bigint") throw raise("Cannot convert a BigInt value to a number");
    const number = isObject(value) ? scripted((item) => +item, undefined, [value]) : +value;
    if (number !== number || number === Infinity || number === -Infinity) return 0n;
    return asUintN(64, BigIntType(trunc(number)));
  };
  const toDOMString = (value) => {
    if (typeof value === "symbol") throw raise("Cannot convert a Symbol value to a string");
    return isObject(value) ? scripted(StringType, undefined, [value]) : StringType(value);
  };
  const observableOf = (receiver) => {
    if (!observables.has(receiver)) throw new TypeErrorType("Illegal invocation");
    return receiver;
  };
  const stateOf = (receiver) => {
    const state = subscribers.get(receiver);
    if (state === undefined) throw new TypeErrorType("Illegal invocation");
    return state;
  };

  // ScriptIterator: GetIterator/IteratorNext as Blink's bindings perform them.
  class ScriptIterator {
    constructor(iterator, next, async) {
      this.iterator = iterator;
      this.nextMethod = next;
      this.async = async;
      this.value = undefined;
    }
    static from(object, async) {
      const method = read(object, async ? asyncIteratorSymbol : iteratorSymbol);
      if (method === undefined || method === null) return null;
      if (typeof method !== "function")
        throw raise(
          async ? "@@asyncIterator must be a callable." : "@@iterator must be a callable.",
        );
      const iterator = scripted(method, object, []);
      if (!isObject(iterator)) throw raise("Iterator object must be an object.");
      return new ScriptIterator(iterator, read(iterator, "next"), async);
    }
    step() {
      if (typeof this.nextMethod !== "function")
        throw raise("Expected next() function on iterator.");
      const result = scripted(this.nextMethod, this.iterator, []);
      if (!isObject(result)) throw raise("Expected iterator.next() to return an Object.");
      if (this.async) {
        this.value = result;
        return true;
      }
      this.value = read(result, "value");
      return !read(result, "done");
    }
  }
  const resolvedWith = (value) =>
    ops.op_value_is_promise(value) ? value : new PromiseType((resolve) => resolve(value));
  const rejectedWith = (reason) => new PromiseType((_, reject) => reject(reason));
  // A promise reaction is a native callback frame without operation context.
  const react = (promise, fulfilled, rejected) =>
    apply(then, promise, [
      (value) => {
        enter(EMPTY);
        try {
          fulfilled(value);
        } finally {
          leave();
        }
      },
      rejected &&
        ((reason) => {
          enter(EMPTY);
          try {
            rejected(reason);
          } finally {
            leave();
          }
        }),
    ]);

  const fromIterable = (iterable, state) => {
    if (aborted(state.signal)) return;
    let iterator;
    try {
      iterator = ScriptIterator.from(iterable, false);
    } catch (error) {
      subscriberError(state, error);
      return;
    }
    if (iterator === null) {
      subscriberError(
        state,
        new TypeErrorType(
          prefix("subscribe", "Observable") + "@@iterator must not be undefined or null",
        ),
      );
      return;
    }
    if (aborted(state.signal)) return;
    const signal = state.signal;
    // An abort closes the iterator with the signal's reason (IteratorClose).
    const release = abortAlgorithm(signal, () => {
      const method = read(iterator.iterator, "return");
      if (method === undefined || method === null) return;
      const frame = top() === SCRIPT || top() === EMPTY ? ABORT : top();
      if (typeof method !== "function") throw raiseIn(frame, "return() function must be callable.");
      const result = scripted(method, iterator.iterator, [reasonOf(signal)]);
      if (!isObject(result)) throw raiseIn(frame, "Expected return() to return an Object.");
    });
    try {
      while (iterator.step()) {
        subscriberNext(state, iterator.value);
        if (aborted(signal)) break;
      }
    } catch (error) {
      release();
      subscriberError(state, error);
      return;
    }
    release();
    subscriberComplete(state);
  };

  const fromAsyncIterable = (iterable, state) => {
    if (aborted(state.signal)) return;
    let iterator;
    try {
      iterator = ScriptIterator.from(iterable, true);
    } catch (error) {
      subscriberError(state, error);
      return;
    }
    if (iterator === null) {
      subscriberError(state, new TypeErrorType("Object must be iterable"));
      return;
    }
    if (aborted(state.signal)) return;
    const signal = state.signal;
    // AsyncIteratorClose: its outcome is a promise nobody observes.
    let release = abortAlgorithm(signal, () => {
      let method;
      try {
        method = read(iterator.iterator, "return");
      } catch (error) {
        rejectedWith(error);
        return;
      }
      if (typeof method !== "function") return;
      const outcome = invoke(method, iterator.iterator, [reasonOf(signal)]);
      if (!outcome.ok) {
        rejectedWith(outcome.value);
        return;
      }
      apply(then, resolvedWith(outcome.value), [
        (value) => {
          if (!isObject(value))
            throw new TypeErrorType(
              prefix("from", "Observable") + "Expected return() to resolve to an Object.",
            );
        },
      ]);
    });
    const clear = () => {
      if (release !== null) release();
      release = null;
    };
    const step = () => {
      if (aborted(signal)) return;
      let pending;
      try {
        iterator.step();
        pending = resolvedWith(iterator.value);
      } catch (error) {
        applyContext(error, "from");
        pending = rejectedWith(error);
      }
      react(
        pending,
        (result) => {
          if (!isObject(result)) {
            clear();
            subscriberError(
              state,
              new TypeErrorType("Expected next() Promise to resolve to an Object"),
            );
            return;
          }
          let done, value;
          try {
            done = read(result, "done");
          } catch (error) {
            clear();
            subscriberError(state, error);
            return;
          }
          if (done) {
            clear();
            subscriberComplete(state);
            return;
          }
          try {
            value = read(result, "value");
          } catch (error) {
            clear();
            subscriberError(state, error);
            return;
          }
          subscriberNext(state, value);
          step();
        },
        (reason) => {
          clear();
          subscriberError(state, reason);
        },
      );
    };
    step();
  };

  const fromPromise = (promise, state) =>
    react(
      promise,
      (value) => {
        subscriberNext(state, value);
        subscriberComplete(state);
      },
      (reason) => subscriberError(state, reason),
    );

  // Observable conversion: Observable, async iterable, iterable, then Promise.
  const toObservable = (value) => {
    if (observables.has(value)) return value;
    if (isObject(value)) {
      const asyncMethod = read(value, asyncIteratorSymbol);
      if (asyncMethod !== undefined && asyncMethod !== null) {
        if (typeof asyncMethod !== "function") throw raise("@@asyncIterator must be a callable.");
        return newObservable((state) => fromAsyncIterable(value, state));
      }
      const method = read(value, iteratorSymbol);
      if (method !== undefined && method !== null) {
        if (typeof method !== "function") throw raise("@@iterator must be a callable.");
        return newObservable((state) => fromIterable(value, state));
      }
    }
    if (ops.op_value_is_promise(value)) return newObservable((state) => fromPromise(value, state));
    throw raise(
      "Cannot convert value to an Observable. Input value must be an Observable, async iterable, iterable, or Promise.",
    );
  };
  // flatMap/switchMap/catch convert a callback's result; a failure carries the
  // innermost frame's context and then the operator's own.
  const convertFor = (value, name, outer) => {
    try {
      return toObservable(value);
    } catch (error) {
      applyContext(error, name);
      subscriberError(outer, error);
      return null;
    }
  };

  // Pass-through observer forwarding to an outer subscriber.
  class Forward extends InternalObserver {
    constructor(outer) {
      super();
      this.outer = outer;
    }
    next(value) {
      subscriberNext(this.outer, value);
    }
    error(error) {
      subscriberError(this.outer, error);
    }
    complete() {
      subscriberComplete(this.outer);
    }
  }
  const pipe = (source, outer, observer) => subscribeTo(source, observer, outer.signal);

  class MapObserver extends Forward {
    constructor(outer, mapper) {
      super(outer);
      this.mapper = mapper;
      this.index = 0;
    }
    next(value) {
      const outcome = invoke(this.mapper, undefined, [value, this.index++]);
      if (outcome.ok) subscriberNext(this.outer, outcome.value);
      else subscriberError(this.outer, outcome.value);
    }
  }
  class FilterObserver extends Forward {
    constructor(outer, predicate) {
      super(outer);
      this.predicate = predicate;
      this.index = 0;
    }
    next(value) {
      const outcome = invoke(this.predicate, undefined, [value, this.index++]);
      if (!outcome.ok) subscriberError(this.outer, outcome.value);
      else if (outcome.value) subscriberNext(this.outer, value);
    }
  }
  class TakeObserver extends Forward {
    constructor(outer, count) {
      super(outer);
      this.count = count;
    }
    next(value) {
      subscriberNext(this.outer, value);
      this.count -= 1n;
      if (!this.count) subscriberComplete(this.outer);
    }
  }
  class DropObserver extends Forward {
    constructor(outer, count) {
      super(outer);
      this.count = count;
    }
    next(value) {
      if (this.count > 0n) {
        this.count -= 1n;
        return;
      }
      subscriberNext(this.outer, value);
    }
  }
  class NotifierObserver extends InternalObserver {
    constructor(outer) {
      super();
      this.outer = outer;
    }
    next() {
      subscriberComplete(this.outer);
    }
    error() {
      subscriberComplete(this.outer);
    }
    complete() {}
  }
  class InnerObserver extends Forward {
    constructor(outer, done) {
      super(outer);
      this.done = done;
    }
    complete() {
      this.done();
    }
  }
  class FlatMapObserver extends Forward {
    constructor(outer, mapper) {
      super(outer);
      this.mapper = mapper;
      this.index = 0;
      this.queue = new List();
      this.activeInner = false;
      this.outerComplete = false;
    }
    next(value) {
      if (this.activeInner) {
        this.queue.add(value);
        return;
      }
      this.activeInner = true;
      this.process(value);
    }
    complete() {
      this.outerComplete = true;
      if (!this.activeInner && !this.queue.length) subscriberComplete(this.outer);
    }
    process(value) {
      const outcome = invoke(this.mapper, undefined, [value, ++this.index]);
      if (!outcome.ok) {
        subscriberError(this.outer, outcome.value);
        return;
      }
      const inner = convertFor(outcome.value, "flatMap", this.outer);
      if (inner === null) return;
      pipe(inner, this.outer, new InnerObserver(this.outer, () => this.innerComplete()));
    }
    innerComplete() {
      if (this.queue.length) {
        this.process(this.queue.shift());
        return;
      }
      this.activeInner = false;
      if (this.outerComplete) subscriberComplete(this.outer);
    }
  }
  class SwitchMapObserver extends Forward {
    constructor(outer, mapper) {
      super(outer);
      this.mapper = mapper;
      this.index = 0;
      this.innerController = null;
      this.outerComplete = false;
    }
    next(value) {
      if (this.innerController !== null) abort(this.innerController, false);
      this.innerController = new Controller();
      const outcome = invoke(this.mapper, undefined, [value, ++this.index]);
      if (!outcome.ok) {
        subscriberError(this.outer, outcome.value);
        return;
      }
      // Blink reports switchMap's conversion failures as `map`.
      const inner = convertFor(outcome.value, "map", this.outer);
      if (inner === null) return;
      const signal = anySignal([
        this.outer.signal,
        apply(controllerSignal, this.innerController, []),
      ]);
      subscribeTo(inner, new InnerObserver(this.outer, () => this.innerComplete()), signal);
    }
    complete() {
      this.outerComplete = true;
      if (this.innerController === null) subscriberComplete(this.outer);
    }
    innerComplete() {
      if (this.outerComplete) {
        subscriberComplete(this.outer);
        return;
      }
      this.innerController = null;
    }
  }
  class CatchObserver extends Forward {
    constructor(outer, handler) {
      super(outer);
      this.handler = handler;
    }
    error(error) {
      const outcome = invoke(this.handler, undefined, [error]);
      if (!outcome.ok) {
        subscriberError(this.outer, outcome.value);
        return;
      }
      const inner = convertFor(outcome.value, "catch", this.outer);
      if (inner !== null) pipe(inner, this.outer, new Forward(this.outer));
    }
  }
  class InspectObserver extends Forward {
    constructor(outer, callbacks, release) {
      super(outer);
      this.callbacks = callbacks;
      this.abortRelease = release;
    }
    // The abort handler is for consumer aborts only, never producer completion.
    reset() {
      if (this.abortRelease !== null) this.abortRelease();
      this.abortRelease = null;
    }
    next(value) {
      if (this.callbacks.next === null) {
        subscriberNext(this.outer, value);
        return;
      }
      const outcome = invoke(this.callbacks.next, undefined, [value]);
      if (!outcome.ok) {
        this.reset();
        subscriberError(this.outer, outcome.value);
      }
      subscriberNext(this.outer, value);
    }
    error(error) {
      this.reset();
      if (this.callbacks.error === null) {
        subscriberError(this.outer, error);
        return;
      }
      const outcome = invoke(this.callbacks.error, undefined, [error]);
      if (!outcome.ok) subscriberError(this.outer, outcome.value);
      subscriberError(this.outer, error);
    }
    complete() {
      this.reset();
      if (this.callbacks.complete === null) {
        subscriberComplete(this.outer);
        return;
      }
      const outcome = invoke(this.callbacks.complete, undefined, []);
      if (!outcome.ok) subscriberError(this.outer, outcome.value);
      subscriberComplete(this.outer);
    }
  }
  class Inspector {
    constructor(next, error, complete, subscribe, abort) {
      this.next = next;
      this.error = error;
      this.complete = complete;
      this.subscribe = subscribe;
      this.abort = abort;
    }
  }

  // Promise-returning operators settle one resolver; an abort rejects it first.
  class Resolver {
    constructor(signal) {
      this.release = null;
      this.aborted = false;
      this.promise = new PromiseType((resolve, reject) => {
        this.resolve = resolve;
        this.reject = reject;
      });
      if (signal === null) return;
      if (aborted(signal)) {
        this.aborted = true;
        this.reject(reasonOf(signal));
        return;
      }
      this.release = abortAlgorithm(signal, () => this.reject(reasonOf(signal)));
    }
    clear() {
      if (this.release !== null) this.release();
      this.release = null;
    }
  }
  class PromiseObserver extends InternalObserver {
    constructor(resolver, controller) {
      super();
      this.resolver = resolver;
      this.controller = controller;
      this.index = 0;
    }
    error(error) {
      this.resolver.clear();
      this.resolver.reject(error);
    }
    fail(error) {
      this.resolver.clear();
      this.resolver.reject(error);
      abort(this.controller, true, error);
    }
    settle(value) {
      this.resolver.clear();
      this.resolver.resolve(value);
      abort(this.controller, false);
    }
  }
  class ToArrayObserver extends PromiseObserver {
    constructor(resolver) {
      super(resolver, null);
      this.values = [];
    }
    next(value) {
      define(this.values, this.values.length, {
        __proto__: null,
        value,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    }
    complete() {
      this.resolver.clear();
      this.resolver.resolve(this.values);
    }
  }
  class LastObserver extends PromiseObserver {
    constructor(resolver) {
      super(resolver, null);
      this.hasValue = false;
      this.last = undefined;
    }
    next(value) {
      this.hasValue = true;
      this.last = value;
    }
    complete() {
      this.resolver.clear();
      if (this.hasValue) this.resolver.resolve(this.last);
      else this.resolver.reject(new RangeErrorType("No values in Observable"));
    }
  }
  class FirstObserver extends PromiseObserver {
    next(value) {
      this.settle(value);
    }
    complete() {
      this.resolver.clear();
      this.resolver.reject(new RangeErrorType("No values in Observable"));
    }
  }
  class ForEachObserver extends PromiseObserver {
    constructor(resolver, controller, visitor) {
      super(resolver, controller);
      this.visitor = visitor;
    }
    next(value) {
      const outcome = invoke(this.visitor, undefined, [value, this.index++]);
      if (outcome.ok) return;
      this.resolver.reject(outcome.value);
      abort(this.controller, true, outcome.value);
    }
    complete() {
      this.resolver.clear();
      this.resolver.resolve(undefined);
    }
  }
  // find, some and every: a predicate decides whether the promise settles early.
  class PredicateObserver extends PromiseObserver {
    constructor(resolver, controller, predicate, kind) {
      super(resolver, controller);
      this.predicate = predicate;
      this.kind = kind;
    }
    next(value) {
      const outcome = invoke(this.predicate, undefined, [value, this.index++]);
      if (!outcome.ok) {
        this.fail(outcome.value);
        return;
      }
      const matches = !!outcome.value;
      if (this.kind === "every") {
        if (!matches) this.settle(false);
      } else if (matches) this.settle(this.kind === "some" ? true : value);
    }
    complete() {
      this.resolver.clear();
      this.resolver.resolve(
        this.kind === "every" ? true : this.kind === "some" ? false : undefined,
      );
    }
  }
  class ReduceObserver extends PromiseObserver {
    constructor(resolver, controller, reducer, hasInitial, initial) {
      super(resolver, controller);
      this.reducer = reducer;
      this.hasAccumulator = hasInitial;
      this.accumulator = initial;
    }
    next(value) {
      if (!this.hasAccumulator) {
        this.hasAccumulator = true;
        this.accumulator = value;
        this.index++;
        return;
      }
      const outcome = invoke(this.reducer, undefined, [this.accumulator, value, this.index++]);
      if (!outcome.ok) {
        this.fail(outcome.value);
        return;
      }
      this.accumulator = outcome.value;
    }
    complete() {
      this.resolver.clear();
      if (this.hasAccumulator) this.resolver.resolve(this.accumulator);
      else this.resolver.reject(new TypeErrorType("Reduce of empty array with no initial value"));
    }
  }
  // first/forEach/some/every/find/reduce subscribe through a private controller
  // joined with the caller's signal; toArray/last use the caller's signal as is.
  const runPromise = (observable, signal, build) => {
    const controller = new Controller();
    const internal = anySignal(
      signal === null
        ? [apply(controllerSignal, controller, [])]
        : [apply(controllerSignal, controller, []), signal],
    );
    const resolver = new Resolver(internal);
    if (!resolver.aborted) subscribeTo(observable, build(resolver, controller), internal);
    return resolver.promise;
  };
  const runDirect = (observable, signal, build) => {
    const resolver = new Resolver(signal);
    if (!resolver.aborted) subscribeTo(observable, build(resolver), signal);
    return resolver.promise;
  };

  const methodDescriptor = (value) => ({
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  });
  // Every exposed operation runs as a binding frame named after itself.
  const operation = (target, iface, name, length, body, promise) => {
    const frame = new Frame(name, iface);
    const native = make(
      promise
        ? function () {
            enter(frame);
            try {
              return apply(body, this, arguments);
            } catch (error) {
              return rejectedWith(error);
            } finally {
              leave();
            }
          }
        : function () {
            enter(frame);
            try {
              return apply(body, this, arguments);
            } finally {
              leave();
            }
          },
      name,
      length,
    );
    define(target, name, methodDescriptor(native));
  };
  const accessor = (target, name, read) =>
    define(target, name, {
      get: make(
        function () {
          return read(stateOf(this));
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  const finishInterface = (constructor, name) => {
    define(constructor.prototype, toStringTag, { value: name, configurable: true });
    define(constructor, "prototype", { writable: false });
    define(realm, name, { value: constructor, writable: true, configurable: true });
  };

  const ObservableConstructor = make(
    function Observable(subscribe) {
      if (new.target === undefined)
        throw new TypeErrorType(
          "Failed to construct 'Observable': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      if (!arguments.length)
        throw new TypeErrorType(
          "Failed to construct 'Observable': 1 argument required, but only 0 present.",
        );
      if (typeof subscribe !== "function")
        throw new TypeErrorType(
          "Failed to construct 'Observable': parameter 1 is not of type 'Function'.",
        );
      observables.set(this, new ObservableState(subscribe, null));
    },
    "Observable",
    1,
    true,
    true,
    false,
  );
  ObservablePrototype = ObservableConstructor.prototype;
  const SubscriberConstructor = make(
    function Subscriber() {
      throw new TypeErrorType(
        new.target === undefined
          ? "Illegal constructor"
          : "Failed to construct 'Subscriber': Illegal constructor",
      );
    },
    "Subscriber",
    0,
    true,
    true,
    false,
  );
  SubscriberPrototype = SubscriberConstructor.prototype;

  const S = SubscriberPrototype;
  operation(S, "Subscriber", "next", 1, function (value) {
    const state = stateOf(this);
    required(arguments, 1);
    subscriberNext(state, value);
  });
  operation(S, "Subscriber", "error", 1, function (error) {
    const state = stateOf(this);
    required(arguments, 1);
    subscriberError(state, error);
  });
  operation(S, "Subscriber", "complete", 0, function () {
    subscriberComplete(stateOf(this));
  });
  operation(S, "Subscriber", "addTeardown", 1, function (teardown) {
    const state = stateOf(this);
    required(arguments, 1);
    addTeardown(state, callback(teardown, 1));
  });
  accessor(S, "active", (state) => state.active);
  accessor(S, "signal", (state) => state.signal);

  const O = ObservablePrototype;
  operation(O, "Observable", "subscribe", 0, function (observer, options) {
    observableOf(this);
    const internal = toObserver(observer);
    subscribeTo(this, internal, toSignal(options));
  });
  const mapping = (name, Observer) =>
    operation(O, "Observable", name, 1, function (mapper) {
      const source = observableOf(this);
      required(arguments, 1);
      callback(mapper, 1);
      return newObservable((outer) => pipe(source, outer, new Observer(outer, mapper)));
    });
  operation(O, "Observable", "takeUntil", 1, function (notifier) {
    const source = observableOf(this);
    required(arguments, 1);
    if (!observables.has(notifier)) throw raise("parameter 1 is not of type 'Observable'.");
    return newObservable((outer) => {
      subscribeTo(notifier, new NotifierObserver(outer), outer.signal);
      if (outer.active) pipe(source, outer, new Forward(outer));
    });
  });
  mapping("map", MapObserver);
  mapping("filter", FilterObserver);
  operation(O, "Observable", "take", 1, function (amount) {
    const source = observableOf(this);
    required(arguments, 1);
    const count = toUint64(amount);
    return newObservable((outer) => {
      if (!count) subscriberComplete(outer);
      else pipe(source, outer, new TakeObserver(outer, count));
    });
  });
  operation(O, "Observable", "drop", 1, function (amount) {
    const source = observableOf(this);
    required(arguments, 1);
    const count = toUint64(amount);
    return newObservable((outer) => pipe(source, outer, new DropObserver(outer, count)));
  });
  mapping("flatMap", FlatMapObserver);
  mapping("switchMap", SwitchMapObserver);
  operation(O, "Observable", "inspect", 0, function (value) {
    const source = observableOf(this);
    let inspector;
    if (typeof value === "function") inspector = new Inspector(value, null, null, null, null);
    else if (value === undefined || value === null)
      inspector = new Inspector(null, null, null, null, null);
    else if (typeof value !== "object")
      throw raise("The provided value is not of type '(ObservableInspector or ObserverCallback)'.");
    else {
      const abortHandler = member(value, "abort", "ObservableInspector"),
        complete = member(value, "complete", "ObservableInspector"),
        error = member(value, "error", "ObservableInspector"),
        next = member(value, "next", "ObservableInspector"),
        subscribe = member(value, "subscribe", "ObservableInspector");
      inspector = new Inspector(next, error, complete, subscribe, abortHandler);
    }
    return newObservable((outer) => {
      if (inspector.subscribe !== null) {
        const outcome = invoke(inspector.subscribe, undefined, []);
        if (!outcome.ok) {
          subscriberError(outer, outcome.value);
          return;
        }
      }
      const release =
        inspector.abort === null
          ? null
          : abortAlgorithm(outer.signal, () =>
              invokeReport(inspector.abort, [reasonOf(outer.signal)]),
            );
      pipe(source, outer, new InspectObserver(outer, inspector, release));
    });
  });
  operation(O, "Observable", "catch", 1, function (handler) {
    const source = observableOf(this);
    required(arguments, 1);
    callback(handler, 1);
    return newObservable((outer) => pipe(source, outer, new CatchObserver(outer, handler)));
  });
  operation(O, "Observable", "finally", 1, function (handler) {
    const source = observableOf(this);
    required(arguments, 1);
    callback(handler, 1);
    return newObservable((outer) => {
      addTeardown(outer, handler);
      pipe(source, outer, new Forward(outer));
    });
  });

  const promiseOperation = (name, length, body) =>
    operation(O, "Observable", name, length, body, true);
  const promiseReceiver = (receiver) => {
    if (!observables.has(receiver)) throw raise("Illegal invocation");
    return receiver;
  };
  promiseOperation("toArray", 0, function (options) {
    const source = promiseReceiver(this);
    return runDirect(source, toSignal(options), (resolver) => new ToArrayObserver(resolver));
  });
  promiseOperation("forEach", 1, function (visitor, options) {
    const source = promiseReceiver(this);
    required(arguments, 1);
    callback(visitor, 1);
    return runPromise(
      source,
      toSignal(options),
      (resolver, controller) => new ForEachObserver(resolver, controller, visitor),
    );
  });
  promiseOperation("first", 0, function (options) {
    const source = promiseReceiver(this);
    return runPromise(
      source,
      toSignal(options),
      (resolver, controller) => new FirstObserver(resolver, controller),
    );
  });
  promiseOperation("last", 0, function (options) {
    const source = promiseReceiver(this);
    return runDirect(source, toSignal(options), (resolver) => new LastObserver(resolver));
  });
  for (const kind of ["some", "every", "find"])
    promiseOperation(kind, 1, function (predicate, options) {
      const source = promiseReceiver(this);
      required(arguments, 1);
      callback(predicate, 1);
      return runPromise(
        source,
        toSignal(options),
        (resolver, controller) => new PredicateObserver(resolver, controller, predicate, kind),
      );
    });
  promiseOperation("reduce", 1, function (reducer) {
    const source = promiseReceiver(this);
    required(arguments, 1);
    callback(reducer, 1);
    // Web IDL drops trailing undefined optional arguments before overload selection.
    let count = arguments.length;
    while (count > 1 && arguments[count - 1] === undefined) count--;
    const hasInitial = count > 1,
      initial = hasInitial ? arguments[1] : undefined;
    const signal = toSignal(count > 2 ? arguments[2] : undefined);
    return runPromise(
      source,
      signal,
      (resolver, controller) =>
        new ReduceObserver(resolver, controller, reducer, hasInitial, initial),
    );
  });
  operation(ObservableConstructor, "Observable", "from", 1, function (value) {
    required(arguments, 1);
    return toObservable(value);
  });
  // Blink lists attributes, then operations, each alphabetically, then constructor.
  const reorder = (target, keys) => {
    for (const key of keys) {
      const value = descriptor(target, key);
      delete target[key];
      define(target, key, value);
    }
  };
  reorder(O, [
    "catch",
    "drop",
    "every",
    "filter",
    "finally",
    "find",
    "first",
    "flatMap",
    "forEach",
    "inspect",
    "last",
    "map",
    "reduce",
    "some",
    "subscribe",
    "switchMap",
    "take",
    "takeUntil",
    "toArray",
    "constructor",
  ]);
  reorder(S, ["active", "signal", "addTeardown", "complete", "error", "next", "constructor"]);
  finishInterface(ObservableConstructor, "Observable");
  finishInterface(SubscriberConstructor, "Subscriber");

  // EventTarget#when: each subscription adds its own listener, removed with
  // the subscriber's signal.
  const probeListener = () => {};
  const isTrusted = (event) => (trustedGetter ? apply(trustedGetter, event, []) : event.isTrusted);
  const dispatchFrame = (event, target) => {
    const frame = top();
    if (frame !== SCRIPT && frame !== EMPTY) return frame;
    if (!isTrusted(event)) return DISPATCH;
    const type = typeGetter ? apply(typeGetter, event, []) : event.type;
    return type === "abort" && isSignal(target) ? ABORT : EMPTY;
  };
  operation(EventTarget.prototype, "EventTarget", "when", 1, function (type, options) {
    const target = this === undefined || this === null ? realm : this;
    try {
      apply(unlisten, target, ["", probeListener]);
    } catch {
      throw new TypeErrorType("Illegal invocation");
    }
    required(arguments, 1);
    const name = toDOMString(type);
    let capture = false,
      passive;
    if (options !== undefined && options !== null) {
      if (!isObject(options))
        throw raise("The provided value is not of type 'ObservableEventListenerOptions'.");
      capture = !!read(options, "capture");
      const value = read(options, "passive");
      if (value !== undefined) passive = !!value;
    }
    return newObservable((state) => {
      if (aborted(state.signal)) return;
      const listener = function (event) {
        enter(dispatchFrame(event, this));
        try {
          subscriberNext(state, event);
        } finally {
          leave();
        }
      };
      const listenerOptions = { __proto__: null, capture, signal: state.signal };
      if (passive !== undefined) listenerOptions.passive = passive;
      apply(listen, target, [name, listener, listenerOptions]);
    });
  });
}
