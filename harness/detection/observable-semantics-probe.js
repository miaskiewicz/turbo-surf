(async () => {
  // Observable/Subscriber semantics as Chrome ships them: producer ref-counting,
  // teardown and abort ordering, every operator, promise-returning operators and
  // their microtask placement, Observable.from conversions and EventTarget.when.
  const out = {};
  let log = [];
  const show = (value) => {
    if (value === undefined) return "undefined";
    if (value === null) return null;
    if (value instanceof DOMException) return ["DOMException", value.name, value.message];
    if (value instanceof Error) return [value.constructor.name, value.message];
    if (value instanceof Event) return ["Event", value.type];
    if (typeof value === "object" || typeof value === "function")
      return Object.prototype.toString.call(value);
    return value;
  };
  const push = (...items) => log.push(items.map(show));
  const reported = [];
  const onError = (event) => {
    reported.push([event.message, show(event.error)]);
    event.preventDefault();
  };
  window.addEventListener("error", onError);
  const task = () => new Promise((done) => setTimeout(done, 0));
  const scenario = async (name, body) => {
    log = [];
    reported.length = 0;
    try {
      await body();
    } catch (error) {
      push("scenario-threw", error);
    }
    await task();
    out[name] = { log, reported: reported.slice() };
  };
  const observer = (label) => ({
    next: (value) => push(label + ".next", value),
    error: (error) => push(label + ".error", error),
    complete: () => push(label + ".complete"),
  });
  const microtasks = (label, count = 4) => {
    let chain = Promise.resolve();
    for (let index = 1; index <= count; index++) chain = chain.then(() => push(label + index));
    return chain;
  };

  await scenario("lifecycle", () => {
    let held;
    const source = new Observable(function (subscriber) {
      held = subscriber;
      push("subscribe", arguments.length, subscriber.active, this === undefined || this === window);
      subscriber.addTeardown(() => push("teardown1", subscriber.active, subscriber.signal.aborted));
      subscriber.addTeardown(() => push("teardown2"));
      subscriber.signal.addEventListener("abort", () =>
        push("abort-event", subscriber.active, subscriber.signal.reason),
      );
      subscriber.next(1);
      subscriber.complete();
      push("after-complete", subscriber.active, subscriber.signal.reason);
      subscriber.next(2);
      subscriber.complete();
      subscriber.addTeardown(() => push("late-teardown"));
      subscriber.error(new Error("after close"));
      push("subscribe-end");
    });
    source.subscribe({
      next(value) {
        "use strict";
        push("next", value, this === undefined, arguments.length);
      },
      complete() {
        "use strict";
        push("complete", this === undefined, arguments.length, held.active);
      },
    });
    push("returned");
  });

  await scenario("errorOrder", () => {
    new Observable((subscriber) => {
      subscriber.addTeardown(() => push("teardown"));
      subscriber.signal.onabort = () => push("abort", subscriber.signal.reason);
      subscriber.error("boom");
      push("post", subscriber.active);
      subscriber.error("again");
    }).subscribe({ error: (error) => push("error-cb", error) });
    new Observable((subscriber) => subscriber.error(new TypeError("unhandled"))).subscribe();
    new Observable((subscriber) => subscriber.error(new RangeError("callback-only"))).subscribe(
      () => push("never"),
    );
  });

  await scenario("throwingCallbacks", () => {
    new Observable(() => {
      throw new Error("thrown");
    }).subscribe({ error: (error) => push("error-cb", error) });
    new Observable(() => {
      throw new Error("thrown-unhandled");
    }).subscribe();
    new Observable((subscriber) => {
      subscriber.complete();
      throw new Error("after-complete-throw");
    }).subscribe({ error: (error) => push("never", error) });
    new Observable((subscriber) => {
      subscriber.next(1);
      subscriber.next(2);
    }).subscribe(() => {
      throw new Error("in-next");
    });
    new Observable((subscriber) => subscriber.complete()).subscribe({
      complete: () => {
        throw new Error("in-complete");
      },
    });
    new Observable((subscriber) => subscriber.error(1)).subscribe({
      error: () => {
        throw new Error("in-error");
      },
    });
    new Observable((subscriber) => {
      subscriber.addTeardown(() => {
        throw new Error("in-teardown");
      });
      subscriber.addTeardown(() => push("teardown-ran"));
      subscriber.complete();
    }).subscribe();
    new Observable((subscriber) => {
      subscriber.complete();
      subscriber.addTeardown(() => {
        throw new Error("late-teardown");
      });
      push("after-late");
    }).subscribe();
    new Observable(class {}).subscribe({
      error: (error) => push("class-error", error.constructor.name),
    });
    new Observable((subscriber) => {
      subscriber.error();
    }).subscribe({ error: (...args) => push("error-args", args.length, args[0]) });
    new Observable((subscriber) => {
      subscriber.next();
    }).subscribe({ next: (...args) => push("next-args", args.length, args[0]) });
  });

  await scenario("refcount", () => {
    let subscriptions = 0,
      current;
    const source = new Observable((subscriber) => {
      subscriptions++;
      current = subscriber;
      push("producer", subscriptions);
      subscriber.addTeardown(() => push("teardown", subscriptions));
      subscriber.signal.addEventListener("abort", () => push("abort", subscriber.signal.reason));
    });
    const first = new AbortController(),
      second = new AbortController();
    source.subscribe(observer("a"), { signal: first.signal });
    source.subscribe(observer("b"), { signal: second.signal });
    source.subscribe(observer("pre"), { signal: AbortSignal.abort("pre-aborted") });
    current.next(1);
    first.abort("first-reason");
    push("after-first", current.active);
    current.next(2);
    second.abort("second-reason");
    push("after-second", current.active);
    source.subscribe(observer("c"));
    push("subscriptions", subscriptions);
    current.complete();
    push("after-complete", current.active);
    let inactive;
    const late = new Observable((subscriber) => {
      inactive = subscriber;
      push("late-producer", subscriber.active, subscriber.signal.aborted, subscriber.signal.reason);
      subscriber.addTeardown(() => push("late-teardown"));
      subscriber.next("x");
    });
    late.subscribe(observer("late"), { signal: AbortSignal.abort("already") });
    push("late-active", inactive.active);
    late.subscribe(observer("late2"));
  });

  await scenario("multicastErrors", () => {
    let current;
    const source = new Observable((subscriber) => {
      current = subscriber;
    });
    source.subscribe({
      next: () => {
        throw new Error("first-observer");
      },
    });
    source.subscribe({
      next: (value) => push("second", value),
      complete: () => push("second-complete"),
    });
    current.next(1);
    current.error(new Error("shared"));
    source.subscribe({ complete: () => push("fresh-complete") });
    current.complete();
  });

  await scenario("abortAlgorithmOrder", () => {
    const controller = new AbortController();
    controller.signal.addEventListener("abort", () => push("user-listener-before"));
    new Observable((subscriber) => {
      subscriber.addTeardown(() => push("teardown", subscriber.signal.reason));
    }).subscribe({}, { signal: controller.signal });
    controller.signal.addEventListener("abort", () => push("user-listener-after"));
    controller.abort("why");
    let inner;
    const outer = new Observable((subscriber) => {
      subscriber.signal.addEventListener("abort", () => push("outer-signal-listener"));
      new Observable((s) => {
        inner = s;
        s.addTeardown(() => push("inner-teardown"));
      }).subscribe({}, { signal: subscriber.signal });
      subscriber.addTeardown(() => push("outer-teardown"));
    });
    const stop = new AbortController();
    outer.subscribe({}, { signal: stop.signal });
    stop.abort();
    push("inner-active", inner.active);
  });

  await scenario("operators", async () => {
    const values = (list) =>
      new Observable((subscriber) => {
        push("source-subscribe");
        subscriber.addTeardown(() => push("source-teardown"));
        for (const value of list) subscriber.next(value);
        subscriber.complete();
      });
    values([1, 2, 3])
      .map((value, index) => value * 10 + index)
      .filter((value, index) => (push("filter", value, index), index !== 1))
      .subscribe(observer("mapFilter"));
    values([1, 2, 3]).take(2).subscribe(observer("take"));
    values([1, 2, 3]).take(0).subscribe(observer("take0"));
    values([1, 2, 3]).drop(2).subscribe(observer("drop"));
    values([1, 2, 3]).drop(5).subscribe(observer("drop5"));
    values([1]).take(-1).subscribe(observer("takeHuge"));
    values([1, 2])
      .map(() => {
        throw new Error("mapper");
      })
      .subscribe(observer("mapThrow"));
    values([1, 2])
      .filter(() => {
        throw new Error("predicate");
      })
      .subscribe(observer("filterThrow"));
    values([1, 2])
      .filter(() => "truthy")
      .subscribe(observer("filterTruthy"));
    let mapThis;
    values([1])
      .map(function () {
        "use strict";
        mapThis = this;
        return arguments.length;
      })
      .subscribe(observer("mapArgs"));
    push("mapThis", mapThis === undefined);
  });

  await scenario("takeUntil", () => {
    let source, notifier;
    const src = new Observable((s) => {
      source = s;
      push("src-subscribe");
      s.addTeardown(() => push("src-teardown"));
    });
    const note = new Observable((s) => {
      notifier = s;
      push("notifier-subscribe");
      s.addTeardown(() => push("notifier-teardown"));
    });
    src.takeUntil(note).subscribe(observer("tu"));
    source.next(1);
    notifier.complete();
    source.next(2);
    const note2 = new Observable((s) => {
      notifier = s;
      s.addTeardown(() => push("notifier2-teardown"));
    });
    new Observable((s) => {
      source = s;
      s.addTeardown(() => push("src2-teardown"));
    })
      .takeUntil(note2)
      .subscribe(observer("tu2"));
    notifier.next("stop");
    source.next("ignored");
    new Observable(() => {
      push("never-subscribed");
    })
      .takeUntil(new Observable((s) => s.next(1)))
      .subscribe(observer("tuSync"));
    new Observable((s) => {
      s.next("a");
    })
      .takeUntil(new Observable((s) => s.error(new Error("notifier-error"))))
      .subscribe(observer("tuError"));
    new Observable((s) => {
      s.next("x");
      s.complete();
      s.addTeardown(() => push("src3-teardown"));
    })
      .takeUntil(new Observable((s) => s.addTeardown(() => push("note3-teardown"))))
      .subscribe(observer("tuSourceDone"));
  });

  await scenario("flatMap", () => {
    let outer;
    const inners = [];
    new Observable((s) => {
      outer = s;
      s.addTeardown(() => push("outer-teardown"));
    })
      .flatMap((value, index) => {
        push("mapper", value, index);
        return new Observable((s) => {
          inners.push(s);
          push("inner-subscribe", value);
          s.addTeardown(() => push("inner-teardown", value));
        });
      })
      .subscribe(observer("fm"));
    outer.next("a");
    outer.next("b");
    inners[0].next("a1");
    inners[0].complete();
    inners[1].next("b1");
    outer.complete();
    push("after-outer-complete");
    inners[1].complete();
    new Observable((s) => {
      s.next(1);
      s.next(2);
      s.complete();
    })
      .flatMap((value) => [value, value * 10])
      .subscribe(observer("fmArrays"));
    new Observable((s) => s.next(1)).flatMap(() => 5).subscribe(observer("fmBad"));
    new Observable((s) => s.next(1))
      .flatMap(() => {
        throw new Error("fm-mapper");
      })
      .subscribe(observer("fmThrow"));
    new Observable((s) => s.next(1))
      .flatMap(() => new Observable((i) => i.error("inner-error")))
      .subscribe(observer("fmInnerError"));
  });

  await scenario("switchMap", () => {
    let outer;
    const inners = [];
    new Observable((s) => {
      outer = s;
    })
      .switchMap((value, index) => {
        push("mapper", value, index);
        return new Observable((s) => {
          inners.push(s);
          s.addTeardown(() => push("inner-teardown", value, s.signal.reason));
        });
      })
      .subscribe(observer("sm"));
    outer.next("a");
    inners[0].next("a1");
    outer.next("b");
    inners[0].next("stale");
    inners[1].next("b1");
    outer.complete();
    push("after-outer-complete");
    inners[1].complete();
    new Observable((s) => s.next(1)).switchMap(() => null).subscribe(observer("smBad"));
    new Observable((s) => {
      s.next(1);
      s.complete();
    })
      .switchMap(() => Promise.resolve("p"))
      .subscribe(observer("smPromise"));
  });

  await scenario("inspect", () => {
    const controller = new AbortController();
    let source;
    const inspected = new Observable((s) => {
      source = s;
      push("source-subscribe");
      s.addTeardown(() => push("source-teardown"));
    }).inspect({
      subscribe: () => push("inspect-subscribe"),
      next: (value) => push("inspect-next", value),
      error: (error) => push("inspect-error", error),
      complete: () => push("inspect-complete"),
      abort: (reason) => push("inspect-abort", reason),
    });
    inspected.subscribe(observer("in"), { signal: controller.signal });
    source.next(1);
    controller.abort("consumer");
    const second = new Observable((s) => {
      s.next(2);
      s.complete();
    }).inspect({
      complete: () => push("inspect2-complete"),
      abort: (reason) => push("inspect2-abort-never", reason),
    });
    second.subscribe(observer("in2"));
    new Observable(() => push("never"))
      .inspect({
        subscribe: () => {
          throw new Error("inspect-subscribe-throw");
        },
      })
      .subscribe(observer("in3"));
    new Observable((s) => {
      s.next(3);
      s.next(4);
    })
      .inspect({
        next: (value) => {
          if (value === 3) throw new Error("inspect-next-throw");
        },
      })
      .subscribe(observer("in4"));
    new Observable((s) => s.error("e"))
      .inspect({
        error: () => {
          throw new Error("inspect-error-throw");
        },
      })
      .subscribe(observer("in5"));
    new Observable((s) => s.complete())
      .inspect({
        complete: () => {
          throw new Error("inspect-complete-throw");
        },
      })
      .subscribe(observer("in6"));
    new Observable((s) => s.next(5))
      .inspect((value) => push("inspect-callback", value))
      .subscribe(observer("in7"));
    new Observable((s) => s.next(6)).inspect().subscribe(observer("in8"));
  });

  await scenario("catchFinally", () => {
    new Observable((s) => {
      s.next(1);
      s.error(new Error("source"));
    })
      .catch((error) => {
        push("catch", error);
        return new Observable((s) => {
          s.next("recovered");
          s.complete();
        });
      })
      .subscribe(observer("c1"));
    new Observable((s) => s.error("x"))
      .catch(() => {
        throw new Error("catch-throw");
      })
      .subscribe(observer("c2"));
    new Observable((s) => s.error("x")).catch(() => 7).subscribe(observer("c3"));
    new Observable((s) => s.error("x")).catch(() => ["a", "b"]).subscribe(observer("c4"));
    new Observable((s) => {
      s.next(1);
      s.complete();
    })
      .catch(() => push("never"))
      .subscribe(observer("c5"));
    new Observable((s) => {
      s.addTeardown(() => push("f-source-teardown"));
      s.next(1);
      s.complete();
    })
      .finally(() => push("finally1"))
      .subscribe(observer("f1"));
    new Observable((s) => s.error("err")).finally(() => push("finally2")).subscribe(observer("f2"));
    const controller = new AbortController();
    new Observable((s) => {
      s.addTeardown(() => push("f3-source-teardown"));
    })
      .finally(() => push("finally3"))
      .subscribe(observer("f3"), { signal: controller.signal });
    controller.abort();
    new Observable((s) => s.complete())
      .finally(() => {
        throw new Error("finally-throw");
      })
      .subscribe(observer("f4"));
  });

  await scenario("promiseOperators", async () => {
    const values = (list, label) =>
      new Observable((s) => {
        push(label + "-subscribe");
        s.addTeardown(() => push(label + "-teardown", s.signal.reason));
        for (const value of list) {
          s.next(value);
          if (!s.active) {
            push(label + "-stopped", value);
            return;
          }
        }
        s.complete();
      });
    const settle = (label, promise) =>
      promise.then(
        (value) => push(label + "-fulfilled", Array.isArray(value) ? JSON.stringify(value) : value),
        (error) => push(label + "-rejected", error),
      );
    const all = [];
    all.push(settle("toArray", values([1, 2], "ta").toArray()));
    microtasks("m", 3);
    all.push(settle("first", values([1, 2], "fi").first()));
    all.push(settle("last", values([1, 2], "la").last()));
    all.push(
      settle(
        "find",
        values([1, 2, 3], "fd").find(
          (value, index) => (push("find-pred", value, index), value === 2),
        ),
      ),
    );
    all.push(
      settle(
        "findNone",
        values([1], "fn").find(() => false),
      ),
    );
    all.push(
      settle(
        "some",
        values([1, 2, 3], "so").some((value) => value === 2),
      ),
    );
    all.push(
      settle(
        "someNone",
        values([1], "sn").some(() => 0),
      ),
    );
    all.push(
      settle(
        "every",
        values([1, 2, 3], "ev").every((value) => value < 2),
      ),
    );
    all.push(
      settle(
        "everyAll",
        values([1], "ea").every(() => 1),
      ),
    );
    all.push(
      settle(
        "reduce",
        values([1, 2, 3], "re").reduce(
          (acc, value, index) => (push("reduce", acc, value, index), acc + value),
        ),
      ),
    );
    all.push(
      settle(
        "reduceInit",
        values([1, 2], "ri").reduce((acc, value, index) => acc + value + index, 100),
      ),
    );
    all.push(
      settle(
        "reduceUndefined",
        values([1, 2], "ru").reduce((acc, value) => String(acc) + value, undefined),
      ),
    );
    all.push(
      settle(
        "reduceEmpty",
        values([], "rz").reduce((acc, value) => acc + value),
      ),
    );
    all.push(
      settle(
        "forEach",
        values([1, 2], "fe").forEach((value, index) => push("forEach-visit", value, index)),
      ),
    );
    all.push(
      settle(
        "forEachThrow",
        values([1, 2], "ft").forEach(() => {
          throw new Error("visitor");
        }),
      ),
    );
    all.push(
      settle(
        "findThrow",
        values([1, 2], "fx").find(() => {
          throw new Error("find-pred");
        }),
      ),
    );
    all.push(
      settle(
        "reduceThrow",
        values([1, 2, 3], "rx").reduce(() => {
          throw new Error("reducer");
        }),
      ),
    );
    all.push(settle("firstEmpty", values([], "fz").first()));
    all.push(settle("lastEmpty", values([], "lz").last()));
    all.push(
      settle("errored", new Observable((s) => s.error(new Error("source-error"))).toArray()),
    );
    all.push(settle("preAborted", values([1], "pa").toArray({ signal: AbortSignal.abort("pre") })));
    all.push(
      settle(
        "preAbortedFirst",
        values([1], "pf").first({ signal: AbortSignal.abort("pre-first") }),
      ),
    );
    push("sync-end");
    await Promise.all(all);
    let held;
    const controller = new AbortController();
    const pendingArray = new Observable((s) => {
      held = s;
      s.addTeardown(() => push("mid-teardown", s.signal.reason));
    }).toArray({ signal: controller.signal });
    held.next(1);
    controller.abort("mid");
    await settle("midAbort", pendingArray);
    const forEachController = new AbortController();
    const visiting = new Observable((s) => {
      held = s;
    }).forEach((value) => push("visit", value), { signal: forEachController.signal });
    held.next("v");
    forEachController.abort(new Error("stop-each"));
    await settle("forEachAbort", visiting);
    const thenable = new Observable((s) =>
      s.next({
        // oxlint-disable-next-line unicorn/no-thenable -- thenable adoption is the subject
        then: (resolve) => {
          push("thenable-then");
          resolve("unwrapped");
        },
      }),
    ).first();
    microtasks("t", 3);
    await settle("firstThenable", thenable);
  });

  await scenario("fromConversions", async () => {
    const iterable = {
      [Symbol.iterator]() {
        push("iterator-called");
        let index = 0;
        return {
          next: () => (
            push("next", index),
            index < 3 ? { value: index++, done: false } : { value: "ignored", done: true }
          ),
          return: (...args) => (push("return", args.length, args[0]), {}),
        };
      },
    };
    Observable.from(iterable).subscribe(observer("iter"));
    const controller = new AbortController();
    Observable.from(iterable).subscribe(
      {
        next: (value) => {
          push("iter2.next", value);
          if (value === 1) controller.abort("enough");
        },
      },
      { signal: controller.signal },
    );
    Observable.from(iterable).take(1).subscribe(observer("iterTake"));
    Observable.from({
      [Symbol.iterator]: () => ({
        next() {
          throw new Error("next-throws");
        },
      }),
    }).subscribe(observer("iterThrow"));
    Observable.from({ [Symbol.iterator]: () => 5 }).subscribe(observer("iterNotObject"));
    Observable.from({ [Symbol.iterator]: () => ({ next: () => 5 }) }).subscribe(
      observer("iterResultNotObject"),
    );
    Observable.from({ [Symbol.iterator]: () => ({ next: 5 }) }).subscribe(
      observer("iterNextNotCallable"),
    );
    let flip = true;
    const flipping = {
      get [Symbol.iterator]() {
        push("iterator-getter");
        return flip ? () => [][Symbol.iterator]() : undefined;
      },
    };
    const flipped = Observable.from(flipping);
    flip = false;
    flipped.subscribe(observer("iterGone"));
    const closeThrows = new AbortController();
    Observable.from({
      [Symbol.iterator]: () => ({
        next: () => ({ value: 1, done: false }),
        return() {
          throw new Error("return-throws");
        },
      }),
    }).subscribe(
      {
        next: () => {
          try {
            closeThrows.abort("x");
          } catch (error) {
            push("abort-threw", error);
          }
        },
      },
      { signal: closeThrows.signal },
    );
    Observable.from(Promise.resolve("resolved")).subscribe(observer("promise"));
    Observable.from(Promise.reject("rejected")).subscribe(observer("rejectedPromise"));
    microtasks("p", 3);
    await task();
    async function* generator() {
      try {
        push("gen-start");
        yield 1;
        yield 2;
        yield 3;
      } finally {
        push("gen-finally");
      }
    }
    Observable.from(generator()).subscribe(observer("async"));
    microtasks("a", 6);
    await task();
    const asyncController = new AbortController();
    Observable.from(generator()).subscribe(
      {
        next: (value) => {
          push("async2.next", value);
          if (value === 1) asyncController.abort("async-stop");
        },
      },
      { signal: asyncController.signal },
    );
    await task();
    const custom = {
      [Symbol.asyncIterator]: () => ({
        next: () => (push("custom-next"), Promise.resolve({ value: "c", done: false })),
      }),
    };
    const customController = new AbortController();
    Observable.from(custom).subscribe(
      {
        next: (value) => {
          push("custom.next", value);
          customController.abort("custom-stop");
        },
      },
      { signal: customController.signal },
    );
    await task();
    Observable.from({
      [Symbol.asyncIterator]: () => ({ next: () => Promise.resolve(5) }),
    }).subscribe(observer("asyncNotObject"));
    Observable.from({
      [Symbol.asyncIterator]: () => ({ next: () => Promise.reject("async-reject") }),
    }).subscribe(observer("asyncReject"));
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next() {
          throw new Error("async-next-throws");
        },
      }),
    }).subscribe(observer("asyncThrow"));
    Observable.from({ [Symbol.asyncIterator]: () => 1 }).subscribe(
      observer("asyncIteratorNotObject"),
    );
    await task();
  });

  await scenario("when", async () => {
    const parent = document.createElement("div");
    const child = document.createElement("span");
    parent.append(child);
    document.body.append(parent);
    const controller = new AbortController();
    parent.addEventListener("ping", () => push("listener-before"));
    parent
      .when("ping")
      .subscribe((event) => push("when-bubble", event.eventPhase, event.target === child), {
        signal: controller.signal,
      });
    parent
      .when("ping", { capture: true })
      .subscribe((event) => push("when-capture", event.eventPhase), { signal: controller.signal });
    parent.addEventListener("ping", () => push("listener-after"));
    child.dispatchEvent(new Event("ping", { bubbles: true }));
    controller.abort();
    child.dispatchEvent(new Event("ping", { bubbles: true }));
    const passive = parent.when("cancel", { passive: true });
    passive.subscribe((event) => {
      event.preventDefault();
      push("passive", event.defaultPrevented);
    });
    const notPrevented = parent.dispatchEvent(new Event("cancel", { cancelable: true }));
    const active = parent.when("cancel2");
    active.subscribe((event) => {
      event.preventDefault();
      push("active", event.defaultPrevented);
    });
    const prevented = parent.dispatchEvent(new Event("cancel2", { cancelable: true }));
    push("dispatch-results", notPrevented, prevented);
    const once = parent.when("tick").take(2);
    once.subscribe(observer("take2"));
    for (let index = 0; index < 3; index++)
      parent.dispatchEvent(new CustomEvent("tick", { detail: index }));
    const target = new EventTarget();
    const list = target
      .when("data")
      .map((event) => event.detail)
      .toArray();
    target.dispatchEvent(new CustomEvent("data", { detail: 1 }));
    const signalSource = new AbortController();
    signalSource.signal
      .when("abort")
      .subscribe((event) => push("signal-when", event.type, signalSource.signal.aborted));
    signalSource.abort();
    const shared = target.when("shared");
    shared.subscribe(() => push("shared-a"));
    shared.subscribe(() => push("shared-b"));
    target.dispatchEvent(new Event("shared"));
    window.when("windowevent").subscribe((event) => push("window-when", event.type));
    window.dispatchEvent(new Event("windowevent"));
    const first = parent.when("first").first();
    parent.dispatchEvent(new Event("first"));
    push("first-dispatched");
    await first.then((event) => push("first-resolved", event.type));
    push(
      "toArray-pending",
      await Promise.race([list.then(() => "settled"), task().then(() => "pending")]),
    );
    window.when("touchstart").subscribe((event) => {
      event.preventDefault();
      push("window-touchstart", event.defaultPrevented);
    });
    window.dispatchEvent(new Event("touchstart", { cancelable: true }));
    window.when("touchstart", { passive: false }).subscribe((event) => {
      event.preventDefault();
      push("window-touchstart-active", event.defaultPrevented);
    });
    window.dispatchEvent(new Event("touchstart", { cancelable: true }));
    const typed = parent.when({ toString: () => "coerced" });
    typed.subscribe((event) => push("coerced-type", event.type));
    parent.dispatchEvent(new Event("coerced"));
    const xhr = new XMLHttpRequest();
    try {
      xhr.when("readystatechange").subscribe((event) => push("xhr-event", event.type));
      xhr.dispatchEvent(new Event("readystatechange"));
    } catch (error) {
      push("xhr-when-threw", error);
    }
    const fromUndefined = EventTarget.prototype.when.call(undefined, "unbound");
    fromUndefined.subscribe(() => push("unbound-window-event"));
    window.dispatchEvent(new Event("unbound"));
    parent.remove();
  });

  // Errors Blink raises itself carry the innermost binding call's context.
  await scenario("bindingContexts", async () => {
    const capture = (label) => ({ error: (error) => push(label, error) });
    Observable.from(Promise.resolve(1))
      .flatMap(() => 5)
      .subscribe(capture("promise-flatMap"));
    Observable.from([1])
      .flatMap(() => 5)
      .subscribe(capture("iterable-flatMap"));
    const target = new EventTarget();
    target
      .when("e")
      .flatMap(() => 5)
      .subscribe(capture("dispatch-flatMap"));
    target.dispatchEvent(new Event("e"));
    const button = document.createElement("button");
    document.body.append(button);
    button
      .when("click")
      .flatMap(() => 5)
      .subscribe(capture("click-flatMap"));
    button.click();
    button.remove();
    const controller = new AbortController();
    controller.signal
      .when("abort")
      .flatMap(() => 5)
      .subscribe(capture("abort-flatMap"));
    controller.abort();
    new Observable((s) => queueMicrotask(() => s.next(1)))
      .flatMap(() => 5)
      .subscribe(capture("microtask-flatMap"));
    new Observable((s) => setTimeout(() => s.next(1)))
      .flatMap(() => 5)
      .subscribe(capture("timer-flatMap"));
    new Observable((s) => s.next(1))
      .flatMap(() => 5)
      .toArray()
      .catch((error) => push("toArray-flatMap", error));
    new Observable((s) => s.next(1))
      .map((x) => x)
      .flatMap(() => 5)
      .subscribe(capture("map-flatMap"));
    new Observable((s) => s.next(1))
      .flatMap(() => new Observable((i) => i.next(2)))
      .flatMap(() => 5)
      .subscribe(capture("nested-flatMap"));
    Observable.from([1])
      .switchMap(() => 5)
      .subscribe(capture("iterable-switchMap"));
    Observable.from({ [Symbol.iterator]: () => ({ next: () => 5 }) }).subscribe(
      capture("iterable-direct"),
    );
    new Observable((s) => s.next(1))
      .flatMap(() => ({ [Symbol.iterator]: () => 5 }))
      .subscribe(capture("inner-iterator"));
    new Observable((s) => s.next(1))
      .flatMap(() => ({
        get [Symbol.asyncIterator]() {
          throw new RangeError("getter");
        },
      }))
      .subscribe(capture("getter-flatMap"));
    new Observable((s) => s.error(1))
      .catch(() => ({ [Symbol.iterator]: 3 }))
      .subscribe(capture("catch-iterator"));
    Observable.from(
      (async function* () {
        yield 1;
      })(),
    )
      .flatMap(() => 5)
      .subscribe(capture("async-flatMap"));
    Observable.from({ [Symbol.asyncIterator]: () => ({ next: () => 5 }) }).subscribe(
      capture("async-next-notobject"),
    );
    Observable.from({ [Symbol.asyncIterator]: () => ({ next: 5 }) }).subscribe(
      capture("async-next-notcallable"),
    );
    let calls = 0;
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next: () => (calls++ ? 5 : Promise.resolve({ value: 1, done: false })),
      }),
    }).subscribe(capture("async-second-notobject"));
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next() {
          throw "str";
        },
      }),
    }).subscribe(capture("async-throw-string"));
    const thrown = new TypeError("te");
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next() {
          throw thrown;
        },
      }),
    }).subscribe({
      error: (error) =>
        push(
          "async-throw-object",
          error === thrown,
          error,
          Object.getOwnPropertyDescriptor(error, "message").enumerable,
        ),
    });
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next() {
          throw { message: 7 };
        },
      }),
    }).subscribe({ error: (error) => push("async-throw-plain", error.message) });
    Observable.from({
      [Symbol.asyncIterator]: () => ({
        next: () =>
          Promise.resolve({
            get done() {
              throw new Error("done-getter");
            },
          }),
      }),
    }).subscribe(capture("async-done-getter"));
    Observable.from({
      [Symbol.iterator]: () => ({
        next: () => ({
          get value() {
            throw new Error("value-getter");
          },
          done: false,
        }),
      }),
    }).subscribe(capture("sync-value-getter"));
    Observable.from({
      [Symbol.iterator]: () => ({
        next: () => ({
          value: 1,
          get done() {
            throw new Error("done-getter");
          },
        }),
      }),
    }).subscribe(capture("sync-done-getter"));
    const order = [];
    Observable.from({
      [Symbol.iterator]: () => ({
        next: () => ({
          get value() {
            order.push("value");
            return 1;
          },
          get done() {
            order.push("done");
            return true;
          },
        }),
      }),
    }).subscribe({});
    push("result-read-order", order.join());
    await task();
  });

  // Closing iterators on abort: IteratorClose synchronously, AsyncIteratorClose
  // with an unobserved promise whose rejection is unhandled.
  await scenario("iteratorClose", async () => {
    const rejections = [];
    const onRejection = (event) => {
      rejections.push(show(event.reason));
      event.preventDefault();
    };
    window.addEventListener("unhandledrejection", onRejection);
    const asyncSource = (returnMethod) => {
      const iterator = { next: () => Promise.resolve({ value: 1, done: false }) };
      if (returnMethod !== undefined) iterator.return = returnMethod;
      return { [Symbol.asyncIterator]: () => iterator };
    };
    const cases = [
      ["noReturn", undefined],
      ["nullReturn", null],
      ["numberReturn", 5],
      ["rejectingReturn", () => Promise.reject("rejected-return")],
      ["primitiveResolution", () => Promise.resolve(5)],
      [
        "throwingReturn",
        () => {
          throw "thrown-return";
        },
      ],
      ["okReturn", (...args) => (push("return-args", args.length, args[0]), Promise.resolve({}))],
    ];
    for (const [label, returnMethod] of cases) {
      const controller = new AbortController();
      Observable.from(asyncSource(returnMethod)).subscribe(
        {
          next: () => {
            push(label + "-next");
            controller.abort("stop-" + label);
          },
        },
        { signal: controller.signal },
      );
      await task();
      await task();
      push(label + "-rejections", JSON.stringify(rejections.splice(0)));
    }
    for (const [label, returnMethod] of [
      ["syncNull", null],
      ["syncNumber", 5],
      ["syncPrimitive", () => 5],
    ]) {
      const controller = new AbortController();
      const iterator = { next: () => ({ value: 1, done: false }), return: returnMethod };
      Observable.from({ [Symbol.iterator]: () => iterator }).subscribe(
        { next: () => controller.abort("x") },
        { signal: controller.signal },
      );
      push(label + "-done");
    }
    window.removeEventListener("unhandledrejection", onRejection);
  });

  window.removeEventListener("error", onError);
  return JSON.stringify(out);
})();
