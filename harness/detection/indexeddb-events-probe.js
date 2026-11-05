(async () => {
  // IndexedDB event dispatch as Chrome 156 ships it (#148): exceptions thrown by
  // request/transaction/database listeners (which abort the transaction), the event
  // path request -> transaction -> database with its capture phase, the microtask
  // checkpoint after each listener, and when a new transaction stops being active.
  const out = {};
  let name = 0;
  const fresh = () => `events-${++name}`;
  const err = (error) => (error ? [error.name, error.message] : null);
  const attempt = (callback) => {
    try {
      callback();
      return "ok";
    } catch (error) {
      return error.name;
    }
  };
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
  const settle = (transaction) =>
    new Promise((resolve) => {
      transaction.addEventListener("complete", () => resolve("complete"));
      transaction.addEventListener("abort", () => resolve(["abort", err(transaction.error)]));
    });
  const open = (db) =>
    new Promise((resolve, reject) => {
      const request = indexedDB.open(db, 1);
      request.onupgradeneeded = () => request.result.createObjectStore("s").put("seed", "dup");
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  const read = (db, key) =>
    new Promise((resolve) => {
      const get = db.transaction("s").objectStore("s").get(key);
      get.onsuccess = () => resolve(get.result ?? null);
    });
  // Every reported exception, kept off the console.
  const reports = [];
  addEventListener("error", (event) => {
    reports.push([event.message, event.cancelable, event.isTrusted]);
    event.preventDefault();
  });

  // Throwing listeners: any listener of a request's success or error event that
  // throws aborts its transaction; later listeners still run.
  {
    const cases = {
      listener: ({ store, log }) => {
        const request = store.put("v", "k");
        request.addEventListener("success", () => {
          log.push("l1");
          throw new Error("listener");
        });
        request.addEventListener("success", () => log.push("l2"));
      },
      handleEvent: ({ store, log }) => {
        store.put("v", "k").addEventListener("success", {
          handleEvent() {
            log.push("handleEvent");
            throw new Error("handleEvent");
          },
        });
      },
      handler: ({ store, log }) => {
        store.put("v", "k").onsuccess = () => {
          log.push("onsuccess");
          throw new Error("handler");
        };
      },
      primitive: ({ store, log }) => {
        store.put("v", "k").addEventListener("success", () => {
          log.push("primitive");
          throw 42;
        });
      },
      nested: ({ store, log }) => {
        store.put("v", "k").addEventListener("success", () => {
          const target = new EventTarget();
          target.addEventListener("x", () => {
            throw new Error("nested");
          });
          target.dispatchEvent(new Event("x"));
          log.push("after-nested");
        });
      },
      transactionListener: ({ transaction, store, log }) => {
        store.add("v", "dup").onerror = (event) => event.preventDefault();
        transaction.addEventListener("error", () => {
          log.push("tx-error");
          throw new Error("transaction");
        });
      },
      databaseListener: ({ db, store, log }) => {
        store.add("v", "dup").onerror = (event) => event.preventDefault();
        db.addEventListener("error", () => {
          log.push("db-error");
          throw new Error("database");
        });
      },
      preventedThenThrown: ({ store, log }) => {
        store.add("v", "dup").addEventListener("error", (event) => {
          event.preventDefault();
          log.push("prevented");
          throw new Error("prevented");
        });
      },
      complete: ({ transaction, store, log }) => {
        store.put("v", "k");
        transaction.addEventListener("complete", () => {
          log.push("complete");
          throw new Error("complete");
        });
      },
    };
    out.throws = {};
    for (const [label, arrange] of Object.entries(cases)) {
      const db = await open(fresh());
      const transaction = db.transaction("s", "readwrite");
      const store = transaction.objectStore("s");
      const log = [];
      reports.length = 0;
      arrange({ db, transaction, store, log });
      const later = store.put("later", "later");
      later.onsuccess = () => log.push("later-success");
      later.onerror = (event) => {
        event.preventDefault();
        log.push(["later-error", err(later.error)]);
      };
      const settled = await settle(transaction);
      await tick();
      out.throws[label] = {
        log,
        settled,
        reports: reports.slice(),
        stored: [await read(db, "k"), await read(db, "later")],
      };
      db.close();
    }
    // A throwing upgradeneeded listener aborts the version change transaction.
    const upgradeName = fresh();
    const upgrade = indexedDB.open(upgradeName, 1);
    const log = [];
    reports.length = 0;
    upgrade.addEventListener("upgradeneeded", () => {
      const transaction = upgrade.transaction;
      upgrade.result.createObjectStore("s");
      transaction.onabort = () => log.push(["abort", err(transaction.error)]);
      throw new Error("upgrade");
    });
    log.push(
      await new Promise((resolve) => {
        upgrade.onsuccess = () => {
          upgrade.result.close();
          resolve("success");
        };
        upgrade.onerror = (event) => {
          event.preventDefault();
          resolve(["error", err(upgrade.error)]);
        };
      }),
    );
    log.push((await indexedDB.databases()).some((entry) => entry.name === upgradeName));
    out.throws.upgradeneeded = { log, reports: reports.slice() };
  }

  // The event path: capture listeners on the database and transaction run before the
  // request's, one event object travels the whole path, and stopPropagation in a
  // capture listener keeps the event from the request.
  {
    const db = await open(fresh());
    const trace = (stop) => {
      const transaction = db.transaction("s", "readwrite");
      const request = transaction.objectStore("s").add("v", "dup");
      const names = new Map([
        [db, "db"],
        [transaction, "tx"],
        [request, "req"],
      ]);
      const events = [];
      const log = [];
      const listen = (target, capture) => {
        const listener = (event) => {
          if (!events.includes(event)) events.push(event);
          log.push([
            names.get(target),
            capture ? "capture" : "bubble",
            event.eventPhase,
            names.get(event.currentTarget),
            names.get(event.target),
            events.indexOf(event),
            event.composedPath().map((item) => names.get(item) ?? "?"),
          ]);
          if (stop && capture && target === transaction) event.stopPropagation();
        };
        target.addEventListener("error", listener, capture);
        return () => target.removeEventListener("error", listener, capture);
      };
      const removers = [db, transaction, request].flatMap((target) => [
        listen(target, false),
        listen(target, true),
      ]);
      request.addEventListener("error", (event) => event.preventDefault());
      return settle(transaction).then((settled) => {
        for (const remove of removers) remove();
        return { log, settled, events: events.length };
      });
    };
    out.path = { full: await trace(false), stopped: await trace(true) };
    db.close();
  }

  // A microtask checkpoint runs after every listener, so a listener's microtasks run
  // before the next listener, on the same target or the next one on the path.
  {
    const db = await open(fresh());
    const transaction = db.transaction("s", "readwrite");
    const store = transaction.objectStore("s");
    const log = [];
    const success = store.put("v", "k");
    const listen = (target, type, label) =>
      target.addEventListener(type, (event) => {
        if (type === "error") event.preventDefault();
        log.push(label);
        Promise.resolve().then(() => log.push([`${label}-micro`, attempt(() => store.get("k"))]));
      });
    listen(success, "success", "s1");
    listen(success, "success", "s2");
    const failure = store.add("v", "dup");
    listen(failure, "error", "e1");
    listen(failure, "error", "e2");
    listen(transaction, "error", "tx");
    log.push(await settle(transaction));
    out.microtasks = log;
    db.close();
  }

  // A new transaction is active until the end of the task that created it: a timer
  // queued earlier in that task already sees it inactive, a microtask does not.
  {
    const db = await open(fresh());
    const log = [];
    let transaction;
    setTimeout(() => log.push(["timer", attempt(() => transaction.objectStore("s").get("k"))]), 0);
    transaction = db.transaction("s");
    Promise.resolve().then(() =>
      log.push(["micro", attempt(() => transaction.objectStore("s").get("k"))]),
    );
    log.push(await settle(transaction));
    await tick();
    out.deactivation = log;
    db.close();
  }
  return JSON.stringify(out);
})();
