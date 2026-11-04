(async () => {
  // IndexedDB as Chrome 156 ships it: open/upgradeneeded/versionchange, object
  // stores, indexes, cursors, key ranges, structured clone of values, auto-commit,
  // abort, the error names/messages and the key ordering rules. Event order is
  // traced with a microtask marker after each event so task boundaries show.
  const out = {};
  let name = 0;
  const fresh = () => `probe-${++name}`;
  const err = (error) => (error ? [error.name, error.message] : null);
  const attempt = (callback) => {
    try {
      const value = callback();
      return ["ok", value === undefined ? "undefined" : value];
    } catch (error) {
      return [error.constructor.name, error.name, error.message];
    }
  };
  const done = (request) =>
    new Promise((resolve) => {
      request.addEventListener("success", () => resolve(["success", request.result]));
      request.addEventListener("error", (event) => {
        event.preventDefault();
        resolve(["error", err(request.error)]);
      });
    });
  const settle = (transaction) =>
    new Promise((resolve) => {
      transaction.addEventListener("complete", () => resolve("complete"));
      transaction.addEventListener("abort", () => resolve(["abort", err(transaction.error)]));
    });
  const open = (db, version, upgrade) =>
    new Promise((resolve, reject) => {
      const request = version === undefined ? indexedDB.open(db) : indexedDB.open(db, version);
      request.onupgradeneeded = (event) => upgrade?.(request.result, request.transaction, event);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

  // Open: request state, event order and the version-change details.
  {
    const log = [];
    const db = fresh();
    const request = indexedDB.open(db, 2);
    log.push([
      "sync",
      request.readyState,
      Object.prototype.toString.call(request),
      attempt(() => request.result),
      attempt(() => request.error),
      request.transaction,
      request.source,
    ]);
    Promise.resolve().then(() => log.push("micro0"));
    await new Promise((resolve) => {
      request.onblocked = () => log.push("blocked");
      request.onupgradeneeded = (event) => {
        const transaction = request.transaction;
        log.push([
          "upgradeneeded",
          event.constructor.name,
          event.oldVersion,
          event.newVersion,
          event.bubbles,
          event.cancelable,
          request.readyState,
          transaction.mode,
          transaction.db === request.result,
          request.result.version,
          request.result.name === db,
          Array.from(transaction.objectStoreNames),
          Object.prototype.toString.call(request.result),
        ]);
        Promise.resolve().then(() => log.push("micro-upgrade"));
        request.result.createObjectStore("s");
        transaction.oncomplete = () => {
          log.push(["upgrade-complete", request.transaction === null]);
          Promise.resolve().then(() => log.push("micro-complete"));
        };
      };
      request.onsuccess = (event) => {
        log.push([
          "success",
          event.constructor.name,
          event.bubbles,
          event.cancelable,
          request.readyState,
          request.transaction,
          request.result.version,
          Array.from(request.result.objectStoreNames),
        ]);
        Promise.resolve().then(() => {
          log.push("micro-success");
          request.result.close();
          resolve();
        });
      };
      request.onerror = () => log.push("error");
    });
    // Reopen at the same version (no upgrade) and without a version.
    const order = [];
    const again = indexedDB.open(db);
    again.onupgradeneeded = () => order.push("upgradeneeded");
    const connection = await new Promise((resolve) => {
      again.onsuccess = () => {
        order.push(["success", again.result.version]);
        resolve(again.result);
      };
    });
    connection.close();
    const lower = indexedDB.open(db, 1);
    order.push(await done(lower));
    order.push(attempt(() => indexedDB.open(db, 0)));
    order.push(attempt(() => indexedDB.open(db, -1)));
    order.push(attempt(() => indexedDB.open(db, 2 ** 53)));
    order.push(attempt(() => indexedDB.open()));
    const upgraded = await open(db, 5, (database, _transaction, event) =>
      order.push(["upgrade", event.oldVersion, event.newVersion]),
    );
    order.push(upgraded.version);
    upgraded.close();
    log.push(order);
    out.open = log;
  }

  // Object store CRUD, keyPath and autoIncrement shapes.
  {
    const rows = [];
    const db = await open(fresh(), 1, (database, transaction) => {
      const plain = database.createObjectStore("plain");
      const path = database.createObjectStore("path", { keyPath: "id" });
      const auto = database.createObjectStore("auto", { autoIncrement: true });
      const both = database.createObjectStore("both", { keyPath: "k", autoIncrement: true });
      const nested = database.createObjectStore("nested", { keyPath: "a.b" });
      const compound = database.createObjectStore("compound", { keyPath: ["x", "y"] });
      rows.push(
        [plain, path, auto, both, nested, compound].map((store) => [
          store.name,
          store.keyPath,
          store.autoIncrement,
          Object.prototype.toString.call(store),
          store.transaction === transaction,
          Array.from(store.indexNames),
        ]),
      );
      rows.push(attempt(() => database.createObjectStore("plain")));
      rows.push(
        attempt(() => database.createObjectStore("bad", { keyPath: "", autoIncrement: true })),
      );
      rows.push(
        attempt(() => database.createObjectStore("bad", { keyPath: ["a"], autoIncrement: true })),
      );
      rows.push(attempt(() => database.createObjectStore("bad", { keyPath: "a b" })));
      rows.push(Array.from(database.objectStoreNames));
      database.createObjectStore("gone");
      database.deleteObjectStore("gone");
      rows.push(attempt(() => database.deleteObjectStore("gone")));
      rows.push(Array.from(database.objectStoreNames));
    });
    rows.push(attempt(() => db.createObjectStore("late")));
    const transaction = db.transaction(
      ["plain", "path", "auto", "both", "nested", "compound"],
      "readwrite",
    );
    rows.push([transaction.mode, transaction.durability, Array.from(transaction.objectStoreNames)]);
    const plain = transaction.objectStore("plain");
    const path = transaction.objectStore("path");
    const auto = transaction.objectStore("auto");
    const both = transaction.objectStore("both");
    const nested = transaction.objectStore("nested");
    const compound = transaction.objectStore("compound");
    rows.push(transaction.objectStore("plain") === plain);
    const results = [];
    const track = (label, request) =>
      request.addEventListener("success", () => results.push([label, request.result]));
    track("add", plain.add({ v: 1 }, "a"));
    track("put", plain.put({ v: 2 }, "a"));
    const duplicate = plain.add({ v: 3 }, "a");
    duplicate.onerror = (event) => {
      results.push(["dup", err(duplicate.error), event.bubbles, event.cancelable]);
      event.preventDefault();
    };
    track("get", plain.get("a"));
    track("getMissing", plain.get("zz"));
    track("pathPut", path.put({ id: 7, v: "x" }));
    track("pathGet", path.get(7));
    rows.push(attempt(() => path.put({ v: "no key" })));
    rows.push(attempt(() => path.put({ id: 1 }, 1)));
    rows.push(attempt(() => plain.put({ v: 1 })));
    rows.push(attempt(() => plain.put(1, {})));
    track("auto1", auto.add("first"));
    track("auto2", auto.add("second"));
    track("autoExplicit", auto.put("explicit", 10));
    track("autoFloat", auto.put("float", 12.5));
    track("auto3", auto.add("third"));
    track("autoAll", auto.getAll());
    track("autoKeys", auto.getAllKeys());
    track("both1", both.add({ v: 1 }));
    track("both2", both.add({ v: 2, k: 50 }));
    track("both3", both.add({ v: 3 }));
    track("bothAll", both.getAll());
    rows.push(attempt(() => both.add(5)));
    track("nested", nested.put({ a: { b: "deep" } }));
    track("nestedGet", nested.get("deep"));
    track("compound", compound.put({ x: 1, y: "two" }));
    track("compoundGet", compound.get([1, "two"]));
    track("count", plain.count());
    track("delete", plain.delete("a"));
    track("countAfter", plain.count());
    track("clear", auto.clear());
    track("autoAfterClear", auto.add("after-clear"));
    const end = await settle(transaction);
    rows.push(results, end);
    rows.push(attempt(() => plain.get("a")));
    db.close();
    out.stores = rows;
  }

  // Key validity, comparison and the cross-type ordering (number < Date < string < binary < array).
  {
    const rows = [];
    const cases = [
      [1, 2],
      [2, 1],
      [1, 1],
      [-Infinity, 0],
      [0, -0],
      [1, "1"],
      ["a", "B"],
      ["a", "ab"],
      ["é", "z"],
      ["😀", "￿"],
      [new Date(5), 10],
      [new Date(5), "a"],
      [new Date(5), new Date(4)],
      ["z", new Uint8Array([0])],
      [new Uint8Array([1]), new Uint8Array([1, 0])],
      [new Uint8Array([2]), new ArrayBuffer(3)],
      [new Uint8Array([1]), []],
      [[1], [1, 2]],
      [[2], [1, 2]],
      [["a"], [1]],
      [[], []],
    ];
    rows.push(cases.map(([left, right]) => indexedDB.cmp(left, right)));
    for (const bad of [NaN, null, undefined, true, {}, new Date(NaN), [NaN], Symbol("s"), () => 1])
      rows.push(attempt(() => indexedDB.cmp(bad, 1)));
    const cyclic = [];
    cyclic.push(cyclic);
    rows.push(attempt(() => indexedDB.cmp(cyclic, 1)));
    rows.push(attempt(() => indexedDB.cmp(1)));
    const db = await open(fresh(), 1, (database) => database.createObjectStore("k"));
    const transaction = db.transaction("k", "readwrite");
    const store = transaction.objectStore("k");
    const keys = [
      [2, "b"],
      "b",
      new Uint8Array([5]),
      -1,
      new Date(100),
      "a",
      [1],
      Infinity,
      -Infinity,
      new Date(-5),
      "",
      [],
      new Uint8Array([]),
      [2],
      "aa",
      0,
    ];
    keys.forEach((key, index) => store.put(index, key));
    const describe = (key) =>
      key instanceof Date
        ? ["date", key.getTime()]
        : key instanceof ArrayBuffer
          ? ["binary", Array.from(new Uint8Array(key))]
          : Array.isArray(key)
            ? ["array", key.map(describe)]
            : [typeof key, typeof key === "number" && !Number.isFinite(key) ? String(key) : key];
    const all = store.getAllKeys();
    const values = store.getAll();
    await settle(transaction);
    rows.push(all.result.map(describe), values.result);
    db.close();
    out.keys = rows;
  }

  // IDBKeyRange construction, includes() and range-limited reads.
  {
    const rows = [];
    const shape = (range) => [
      Object.prototype.toString.call(range),
      range.lower,
      range.upper,
      range.lowerOpen,
      range.upperOpen,
    ];
    rows.push(shape(IDBKeyRange.only(3)));
    rows.push(shape(IDBKeyRange.lowerBound(2)));
    rows.push(shape(IDBKeyRange.lowerBound(2, true)));
    rows.push(shape(IDBKeyRange.upperBound("x", true)));
    rows.push(shape(IDBKeyRange.bound(1, 5, true, false)));
    rows.push(attempt(() => IDBKeyRange.bound(5, 1)));
    rows.push(attempt(() => IDBKeyRange.bound(1, 1, true, false)));
    rows.push(attempt(() => IDBKeyRange.only(NaN)));
    rows.push(attempt(() => IDBKeyRange.lowerBound({})));
    rows.push(attempt(() => new IDBKeyRange()));
    const range = IDBKeyRange.bound(2, 4, false, true);
    rows.push([1, 2, 3, 4, 5].map((key) => range.includes(key)));
    rows.push(attempt(() => range.includes({})));
    const db = await open(fresh(), 1, (database) => database.createObjectStore("r"));
    const transaction = db.transaction("r", "readwrite");
    const store = transaction.objectStore("r");
    for (let key = 1; key <= 8; key++) store.put(`v${key}`, key);
    const reads = [
      store.getAll(IDBKeyRange.bound(2, 6)),
      store.getAll(IDBKeyRange.bound(2, 6, true, true)),
      store.getAll(IDBKeyRange.lowerBound(5), 2),
      store.getAll(null, 3),
      store.getAll(4),
      store.getAll(undefined, 0),
      store.getAllKeys(IDBKeyRange.upperBound(3)),
      store.count(IDBKeyRange.lowerBound(6, true)),
      store.get(IDBKeyRange.lowerBound(3, true)),
      store.getKey(IDBKeyRange.lowerBound(3, true)),
      store.delete(IDBKeyRange.bound(3, 5)),
      store.getAllKeys(),
      store.getAllRecords({ query: IDBKeyRange.lowerBound(2), count: 2, direction: "prev" }),
      store.getAllRecords(),
    ];
    rows.push(attempt(() => store.getAll(undefined, -1)));
    rows.push(attempt(() => store.get()));
    rows.push(attempt(() => store.delete()));
    await settle(transaction);
    rows.push(
      reads.map((request) =>
        Array.isArray(request.result) && typeof request.result[0] === "object"
          ? request.result.map((record) => [
              Object.prototype.toString.call(record),
              record.key,
              record.primaryKey,
              record.value,
            ])
          : request.result,
      ),
    );
    db.close();
    out.ranges = rows;
  }

  // Indexes: unique, multiEntry, compound key paths and lookups.
  {
    const rows = [];
    const db = await open(fresh(), 1, (database) => {
      const store = database.createObjectStore("people", { keyPath: "id" });
      const byName = store.createIndex("name", "name");
      const byMail = store.createIndex("mail", "mail", { unique: true });
      const byTag = store.createIndex("tags", "tags", { multiEntry: true });
      const byPair = store.createIndex("pair", ["name", "age"]);
      rows.push(
        [byName, byMail, byTag, byPair].map((index) => [
          index.name,
          index.keyPath,
          index.unique,
          index.multiEntry,
          index.objectStore === store,
          Object.prototype.toString.call(index),
        ]),
      );
      rows.push(Array.from(store.indexNames));
      rows.push(attempt(() => store.createIndex("name", "x")));
      rows.push(attempt(() => store.createIndex("multi", ["a", "b"], { multiEntry: true })));
      store.createIndex("gone", "g");
      store.deleteIndex("gone");
      rows.push(attempt(() => store.index("gone")));
    });
    const transaction = db.transaction("people", "readwrite");
    const store = transaction.objectStore("people");
    rows.push(store.index("name") === store.index("name"));
    store.put({ id: 1, name: "ann", mail: "a@x", tags: ["x", "y", "x"], age: 30 });
    store.put({ id: 2, name: "bob", mail: "b@x", tags: "y", age: 20 });
    store.put({ id: 3, name: "ann", mail: "c@x", tags: [], age: 25 });
    store.put({ id: 4, mail: "d@x", tags: ["z", {}], age: 40 });
    const clash = store.put({ id: 5, name: "eve", mail: "a@x" });
    clash.onerror = (event) => event.preventDefault();
    const name = store.index("name");
    const tags = store.index("tags");
    const pair = store.index("pair");
    const reads = [
      name.get("ann"),
      name.getKey("ann"),
      name.getAll("ann"),
      name.getAllKeys(),
      name.count(),
      name.count("ann"),
      tags.getAllKeys("y"),
      tags.getAllKeys(),
      tags.count(),
      pair.getAll(IDBKeyRange.bound(["ann", 0], ["ann", 26])),
      store.index("mail").get("b@x"),
      name.get("nobody"),
      name.getAllRecords({ query: "ann" }),
    ];
    const end = await settle(transaction);
    rows.push(
      reads.map((request) =>
        request === reads.at(-1)
          ? request.result.map((record) => [record.key, record.primaryKey, record.value.id])
          : request.result,
      ),
      err(clash.error),
      end,
    );
    db.close();
    out.indexes = rows;
  }

  // Cursors: directions, unique iteration, continue/advance/continuePrimaryKey, update/delete.
  {
    const rows = [];
    const db = await open(fresh(), 1, (database) => {
      const store = database.createObjectStore("c");
      store.createIndex("v", "v");
    });
    const seed = db.transaction("c", "readwrite");
    const values = { 1: "b", 2: "a", 3: "b", 4: "c", 5: "a", 6: "d" };
    for (const [key, v] of Object.entries(values)) seed.objectStore("c").put({ v }, Number(key));
    await settle(seed);
    const walk = (source, ...args) =>
      new Promise((resolve) => {
        const seen = [];
        const request = source.openCursor(...args);
        request.onsuccess = () => {
          const cursor = request.result;
          if (!cursor) return resolve(seen);
          seen.push([cursor.key, cursor.primaryKey, cursor.value.v, cursor.direction]);
          cursor.continue();
        };
      });
    const read = db.transaction("c");
    const store = read.objectStore("c");
    const index = store.index("v");
    rows.push(
      await Promise.all([
        walk(store),
        walk(store, null, "prev"),
        walk(store, IDBKeyRange.bound(2, 4)),
        walk(index),
        walk(index, null, "prev"),
        walk(index, null, "nextunique"),
        walk(index, null, "prevunique"),
        walk(index, "b"),
      ]),
    );
    rows.push(attempt(() => store.openCursor(null, "sideways")));
    // Cursor shape, continue(key), advance(n), continuePrimaryKey and the errors.
    const steps = await new Promise((resolve) => {
      const seen = [];
      const request = index.openCursor();
      let step = 0;
      request.onsuccess = () => {
        const cursor = request.result;
        if (!cursor) return resolve(seen);
        seen.push([cursor.key, cursor.primaryKey]);
        step++;
        if (step === 1) {
          seen.push([
            Object.prototype.toString.call(cursor),
            cursor.source === index,
            cursor.request === request,
            request.readyState,
          ]);
          cursor.continue("b");
          seen.push(attempt(() => cursor.continue()));
        } else if (step === 2) {
          seen.push(attempt(() => cursor.continue("a")));
          cursor.continuePrimaryKey("b", 3);
        } else if (step === 3) {
          seen.push(attempt(() => cursor.advance(0)));
          cursor.advance(2);
        } else cursor.continue();
      };
    });
    rows.push(steps);
    const keyOnly = await new Promise((resolve) => {
      const request = store.openKeyCursor(null, "prev");
      const seen = [];
      request.onsuccess = () => {
        const cursor = request.result;
        if (!cursor) return resolve(seen);
        seen.push([
          cursor.key,
          cursor.primaryKey,
          "value" in cursor,
          Object.prototype.toString.call(cursor),
        ]);
        cursor.continue();
      };
    });
    rows.push(keyOnly);
    rows.push(attempt(() => store.openCursor().result));
    await settle(read);
    // update/delete through a readwrite cursor, and readonly rejection.
    const write = db.transaction("c", "readwrite");
    const writeStore = write.objectStore("c");
    const edits = [];
    const cursorRequest = writeStore.openCursor();
    cursorRequest.onsuccess = () => {
      const cursor = cursorRequest.result;
      if (!cursor) return;
      if (cursor.key === 2) {
        const update = cursor.update({ v: "A", extra: true });
        update.onsuccess = () => edits.push(["updated", update.result]);
      }
      if (cursor.key === 3)
        cursor.delete().onsuccess = (event) => edits.push(["deleted", event.target.result]);
      if (cursor.key === 4) edits.push(attempt(() => cursor.update({ v: "c" }, 1)));
      cursor.continue();
    };
    const final = writeStore.getAll();
    await settle(write);
    rows.push(edits, final.result);
    const readonly = db.transaction("c");
    const roRequest = readonly.objectStore("c").openCursor();
    const roResult = await new Promise((resolve) => {
      roRequest.onsuccess = () =>
        resolve([
          attempt(() => roRequest.result.update({})),
          attempt(() => roRequest.result.delete()),
        ]);
    });
    rows.push(roResult);
    await settle(readonly);
    db.close();
    out.cursors = rows;
  }

  // Structured clone: values are copies, built-ins round-trip, non-cloneables throw.
  {
    const rows = [];
    const db = await open(fresh(), 1, (database) => database.createObjectStore("v"));
    const transaction = db.transaction("v", "readwrite");
    const store = transaction.objectStore("v");
    const value = {
      date: new Date(42),
      map: new Map([[1, "one"]]),
      set: new Set(["s"]),
      regex: /a+/gi,
      bytes: new Uint8Array([1, 2, 3]),
      big: 10n,
      nested: { list: [1, , 3] },
      undef: undefined,
    };
    value.self = value;
    store.put(value, "complex");
    value.date.setTime(0);
    value.nested.list.push(9);
    rows.push(attempt(() => store.put({ f() {} }, "fn")));
    rows.push(attempt(() => store.put(Symbol("s"), "sym")));
    rows.push(attempt(() => store.put(document.body, "node")));
    const first = store.get("complex");
    const second = store.get("complex");
    await settle(transaction);
    const copy = first.result;
    rows.push([
      copy !== second.result,
      copy.self === copy,
      copy.date instanceof Date,
      copy.date.getTime(),
      copy.map instanceof Map,
      Array.from(copy.map),
      Array.from(copy.set),
      String(copy.regex),
      Object.prototype.toString.call(copy.bytes),
      Array.from(copy.bytes),
      typeof copy.big,
      copy.nested.list.length,
      1 in copy.nested.list,
      "undef" in copy,
      Object.keys(copy),
    ]);
    db.close();
    out.clone = rows;
  }

  // Auto-commit and transaction lifetime: activity windows, request/event ordering.
  {
    const log = [];
    const db = await open(fresh(), 1, (database) => database.createObjectStore("t"));
    const transaction = db.transaction("t", "readwrite");
    const store = transaction.objectStore("t");
    const first = store.put("one", 1);
    const second = store.put("two", 2);
    first.onsuccess = () => {
      log.push(["first", first.readyState, second.readyState]);
      Promise.resolve().then(() => {
        log.push("first-micro");
        const nested = store.get(1);
        nested.onsuccess = () => log.push(["nested", nested.result]);
      });
    };
    second.onsuccess = () => log.push("second");
    transaction.oncomplete = (event) => {
      log.push([
        "complete",
        event.constructor.name,
        event.bubbles,
        event.cancelable,
        transaction.error,
      ]);
    };
    log.push(["sync", first.readyState, attempt(() => first.result)]);
    Promise.resolve().then(() => log.push("micro-after-create"));
    setTimeout(() => {
      log.push(["timeout", attempt(() => store.get(1))]);
    }, 0);
    await settle(transaction);
    await tick();
    log.push(attempt(() => store.get(1)));
    // A transaction with no requests commits too.
    const empty = db.transaction("t");
    log.push(await settle(empty));
    // commit() ends the transaction early; later requests throw.
    const early = db.transaction("t", "readwrite");
    early.objectStore("t").put("three", 3);
    early.commit();
    log.push(attempt(() => early.objectStore("t").put("four", 4)));
    log.push(attempt(() => early.commit()));
    log.push(await settle(early));
    const readonly = db.transaction("t", "readonly");
    log.push(attempt(() => readonly.objectStore("t").put("x", 9)));
    log.push(attempt(() => readonly.objectStore("missing")));
    log.push(attempt(() => db.transaction("missing")));
    log.push(attempt(() => db.transaction([])));
    log.push(attempt(() => db.transaction("t", "versionchange")));
    await settle(readonly);
    db.close();
    log.push(attempt(() => db.transaction("t")));
    out.lifetime = log;
  }

  // Abort: request errors, the abort event, rollback, and error-driven abort.
  {
    const log = [];
    const db = await open(fresh(), 1, (database) => database.createObjectStore("a"));
    const seed = db.transaction("a", "readwrite");
    seed.objectStore("a").put("kept", 1);
    await settle(seed);
    const transaction = db.transaction("a", "readwrite");
    const store = transaction.objectStore("a");
    const put = store.put("rolled-back", 1);
    const add = store.add("new", 2);
    put.onsuccess = () => log.push("put-success");
    put.onerror = (event) =>
      log.push(["put-error", err(put.error), event.bubbles, event.cancelable, put.readyState]);
    add.onerror = () => log.push(["add-error", err(add.error)]);
    transaction.onerror = (event) => log.push(["tx-error", event.target === transaction]);
    transaction.onabort = (event) =>
      log.push(["tx-abort", event.constructor.name, event.bubbles, err(transaction.error)]);
    db.onabort = (event) => log.push(["db-abort", event.target === transaction]);
    transaction.abort();
    log.push(["after-abort", attempt(() => put.result), attempt(() => put.error), put.readyState]);
    log.push(attempt(() => store.get(1)));
    log.push(attempt(() => transaction.abort()));
    await tick();
    await tick();
    const check = db.transaction("a");
    const get = check.objectStore("a").getAll();
    await settle(check);
    log.push(get.result);
    // An unhandled request error aborts the transaction with that error. The
    // database's abort handler is cleared so settle()'s resolution does not race
    // with listeners later in the same dispatch.
    db.onabort = null;
    const failing = db.transaction("a", "readwrite");
    const order = [];
    const dup = failing.objectStore("a").add("again", 1);
    const after = failing.objectStore("a").put("after", 3);
    dup.onerror = (event) => order.push(["dup-error", event.eventPhase, event.defaultPrevented]);
    after.onerror = () => order.push(["after-error", err(after.error)]);
    failing.addEventListener("error", (event) =>
      order.push(["tx-error", event.target === dup, event.eventPhase]),
    );
    db.onerror = (event) => order.push(["db-error", event.target === dup]);
    order.push(await settle(failing));
    log.push(order);
    // A throwing success handler aborts too.
    const throwing = db.transaction("a", "readwrite");
    const request = throwing.objectStore("a").put("t", 4);
    const reported = [];
    const handler = (event) => {
      event.preventDefault();
      reported.push(event.message);
    };
    window.addEventListener("error", handler);
    request.onsuccess = () => {
      throw new Error("boom");
    };
    log.push(await settle(throwing), reported);
    window.removeEventListener("error", handler);
    db.onabort = null;
    db.onerror = null;
    db.close();
    // Aborting the upgrade transaction fails the open and leaves no database.
    const abortName = fresh();
    const upgrade = indexedDB.open(abortName, 3);
    const upgradeLog = [];
    upgrade.onupgradeneeded = () => {
      upgrade.result.createObjectStore("x");
      upgrade.transaction.onabort = () => upgradeLog.push("upgrade-abort");
      upgrade.transaction.abort();
    };
    upgradeLog.push(await done(upgrade));
    const listed = (await indexedDB.databases()).some((entry) => entry.name === abortName);
    upgradeLog.push(listed);
    log.push(upgradeLog);
    out.abort = log;
  }

  // Version change between connections, blocked, deleteDatabase and databases().
  {
    const log = [];
    const db = fresh();
    const first = await open(db, 1, (database) => database.createObjectStore("s"));
    first.onversionchange = (event) => {
      log.push(["versionchange", event.constructor.name, event.oldVersion, event.newVersion]);
      first.close();
    };
    const second = await open(db, 2, (_database, _transaction, event) =>
      log.push(["upgrade", event.oldVersion]),
    );
    log.push(second.version);
    // A connection that stays open blocks the next upgrade until it closes.
    second.onversionchange = () => log.push("second-versionchange");
    const third = indexedDB.open(db, 3);
    third.onblocked = (event) => {
      log.push(["blocked", event.constructor.name, event.oldVersion, event.newVersion]);
      second.close();
    };
    third.onupgradeneeded = () => log.push("third-upgrade");
    await done(third);
    third.result.close();
    const list = await indexedDB.databases();
    log.push(
      list
        .filter((entry) => entry.name === db)
        .map((entry) => [entry.name === db, entry.version, Object.keys(entry)]),
    );
    const removal = indexedDB.deleteDatabase(db);
    const removed = await new Promise((resolve) => {
      removal.onsuccess = (event) =>
        resolve([
          event.constructor.name,
          event.oldVersion,
          event.newVersion,
          removal.result,
          removal.readyState,
        ]);
    });
    log.push(removed);
    const missing = indexedDB.deleteDatabase("never-existed");
    log.push(
      await new Promise((resolve) => {
        missing.onsuccess = (event) => resolve([event.oldVersion, event.newVersion]);
      }),
    );
    log.push((await indexedDB.databases()).some((entry) => entry.name === db));
    out.versionchange = log;
  }

  // Interface surface.
  out.surface = [
    typeof IDBFactory,
    Object.prototype.toString.call(indexedDB),
    indexedDB instanceof IDBFactory,
    attempt(() => new IDBFactory()),
    attempt(() => new IDBRequest()),
    Object.getPrototypeOf(IDBOpenDBRequest.prototype) === IDBRequest.prototype,
    Object.getPrototypeOf(IDBRequest.prototype) === EventTarget.prototype,
    Object.getPrototypeOf(IDBCursorWithValue.prototype) === IDBCursor.prototype,
    Object.getPrototypeOf(IDBVersionChangeEvent.prototype) === Event.prototype,
    (() => {
      const event = new IDBVersionChangeEvent("versionchange", { oldVersion: 1, newVersion: 2 });
      return [event.type, event.oldVersion, event.newVersion, event.dataLoss];
    })(),
    new IDBVersionChangeEvent("x").newVersion,
    Object.getOwnPropertyNames(IDBFactory.prototype).sort(),
    Object.getOwnPropertyNames(IDBDatabase.prototype).sort(),
    Object.getOwnPropertyNames(IDBObjectStore.prototype).sort(),
    Object.getOwnPropertyNames(IDBIndex.prototype).sort(),
    Object.getOwnPropertyNames(IDBCursor.prototype).sort(),
    Object.getOwnPropertyNames(IDBTransaction.prototype).sort(),
    Object.getOwnPropertyNames(IDBKeyRange).sort(),
    IDBFactory.prototype.open.length,
    IDBObjectStore.prototype.put.length,
    String(IDBFactory.prototype.open),
  ];
  return JSON.stringify(out);
})();
