import assert from "node:assert/strict";
import test from "node:test";
import {
  historyFixture,
  popup,
  assign,
  guestTraversal,
  assertSameBack,
} from "./history-group-fixture.mjs";

test("document-group back restores stored state before same-document traversal and exhaustion", async () => {
  await historyFixture(async ({ root, base, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => history.pushState({ step: 1 }, "", "/spa"));
    await assign(page, "/real");
    assert.equal((await page.goBack({ waitUntil: "load", timeout: 5000 })).status(), 200);
    assert.equal(page.url(), `${base}/spa`);
    assert.deepEqual(await page.evaluate(() => history.state), { step: 1 });
    await page.evaluate(() => {
      window.restoredMarker = marker;
    });
    await assertSameBack(page, "/one");
    assert.equal(await page.evaluate(() => marker === restoredMarker), true);
    assert.equal(await page.evaluate(() => history.state), null);
    assert.deepEqual(await page.evaluate(() => popEvents), ["/one"]);
    const before = requests.length;
    assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(page.url(), `${base}/one`);
    assert.equal(requests.length, before);
    assert.deepEqual(requests, ["/host", "/one", "/real", "/spa"]);
  });
});

test("restored groups retain duplicate entry identity and replacement/forward semantics", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => {
      history.pushState({ n: 1 }, "", "/same");
      history.pushState({ n: 2 }, "", "/same");
      history.replaceState({ n: 3 }, "", "/same");
    });
    await assign(page, "/real");
    await page.goBack({ waitUntil: "load", timeout: 5000 });
    assert.deepEqual(await page.evaluate(() => history.state), { n: 3 });
    await assertSameBack(page, "/same");
    assert.deepEqual(await page.evaluate(() => history.state), { n: 1 });
    await page.evaluate(() => history.replaceState({ n: 4 }, "", "/replacement"));
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.deepEqual(await page.evaluate(() => history.state), { n: 3 });
    await assertSameBack(page, "/replacement");
    assert.deepEqual(await page.evaluate(() => history.state), { n: 4 });
    await page.evaluate(() => history.pushState({ n: 5 }, "", "/branch"));
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(new URL(page.url()).pathname, "/branch");
    await assertSameBack(page, "/replacement");
    await assertSameBack(page, "/one");
    assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
    assert.deepEqual(requests, ["/host", "/one", "/real", "/same"]);
  });
});

test("guest back/forward/go cross document groups while same-group traversal remains live", async () => {
  await historyFixture(async ({ root, base, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => history.pushState({ n: 1 }, "", "/spa"));
    await assign(page, "/two");
    await page.evaluate(() => history.pushState({ n: 2 }, "", "/two-spa"));
    await assign(page, "/three");
    await guestTraversal(page, "go", -2);
    assert.equal(page.url(), `${base}/two`);
    assert.equal(await page.evaluate(() => history.state), null);
    await guestTraversal(page, "back");
    assert.equal(page.url(), `${base}/spa`);
    assert.deepEqual(await page.evaluate(() => history.state), { n: 1 });
    await page.evaluate(() => {
      window.restoredMarker = marker;
    });
    await assertSameBack(page, "/one");
    assert.equal(await page.evaluate(() => marker === restoredMarker), true);
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.deepEqual(await page.evaluate(() => history.state), { n: 1 });
    await guestTraversal(page, "forward");
    assert.equal(page.url(), `${base}/two`);
    assert.equal(await page.evaluate(() => history.state), null);
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(page.url(), `${base}/two-spa`);
    assert.deepEqual(await page.evaluate(() => history.state), { n: 2 });
    assert.deepEqual(requests, ["/host", "/one", "/two", "/three", "/two", "/spa", "/two"]);
  });
});

test("document-group state preserves cyclic data BigInt and typed arrays in the destination realm", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => {
      const value = { integer: 9007199254740993n, bytes: new Uint8Array([0, 255, 128]) };
      value.self = value;
      history.pushState(value, "", "/state");
    });
    await assign(page, "/real");
    await page.goBack({ waitUntil: "load", timeout: 5000 });
    assert.deepEqual(
      await page.evaluate(() => ({
        integer: history.state.integer.toString(),
        bytes: Array.from(history.state.bytes),
        cycle: history.state.self === history.state,
        localObject: Object.getPrototypeOf(history.state) === Object.prototype,
        localArray: Object.getPrototypeOf(history.state.bytes) === Uint8Array.prototype,
      })),
      {
        integer: "9007199254740993",
        bytes: [0, 255, 128],
        cycle: true,
        localObject: true,
        localArray: true,
      },
    );
    await assertSameBack(page, "/one");
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(await page.evaluate(() => history.state.self === history.state), true);
    assert.deepEqual(requests, ["/host", "/one", "/real", "/state"]);
  });
});

test("stored CryptoKey and geometry materialize after destination clone adapters", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(async () => {
      const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, false, [
        "encrypt",
        "decrypt",
      ]);
      history.pushState({ key, matrix: new DOMMatrix([1, 2, 3, 4, 5, 6]) }, "", "/native-state");
    });
    await assign(page, "/real");
    await page.goBack({ waitUntil: "load", timeout: 5000 });
    assert.deepEqual(
      await page.evaluate(async () => {
        const { key, matrix } = history.state;
        const iv = new Uint8Array(12);
        const encrypted = await crypto.subtle.encrypt(
          { name: "AES-GCM", iv },
          key,
          new Uint8Array([7, 9]),
        );
        const decrypted = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, encrypted);
        return {
          key: key instanceof CryptoKey,
          keyPrototype: Object.getPrototypeOf(key) === CryptoKey.prototype,
          matrix: matrix instanceof DOMMatrix,
          matrixPrototype: Object.getPrototypeOf(matrix) === DOMMatrix.prototype,
          extractable: key.extractable,
          algorithm: key.algorithm.name,
          coefficients: [matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f],
          bytes: Array.from(new Uint8Array(decrypted)),
        };
      }),
      {
        key: true,
        keyPrototype: true,
        matrix: true,
        matrixPrototype: true,
        extractable: false,
        algorithm: "AES-GCM",
        coefficients: [1, 2, 3, 4, 5, 6],
        bytes: [7, 9],
      },
    );
    assert.deepEqual(requests, ["/host", "/one", "/real", "/native-state"]);
  });
});

test("storage serialization rejects SAB and Wasm atomically without changing generic clone or getter exceptions", async () => {
  await historyFixture(
    async ({ root, requests }) => {
      const page = await popup(root);
      assert.equal(await page.evaluate(() => crossOriginIsolated), true);
      await page.evaluate(() => {
        history.pushState({ n: 1 }, "", "/first");
        history.pushState({ n: 2 }, "", "/second");
      });
      await assertSameBack(page, "/first");
      assert.deepEqual(
        await page.evaluate(() => {
          const shared = new SharedArrayBuffer(8);
          const sharedClone = structuredClone(shared);
          new Uint8Array(shared)[0] = 37;
          const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));
          const moduleClone = structuredClone(module);
          const savedState = history.state;
          const errors = [];
          for (const operation of ["pushState", "replaceState"]) {
            for (const value of [shared, module]) {
              try {
                history[operation]({ value }, "", "/forbidden");
                errors.push("committed");
              } catch (error) {
                errors.push({
                  name: error.name,
                  local: Object.getPrototypeOf(error) === DOMException.prototype,
                });
              }
            }
          }
          const sentinel = new Error("history getter sentinel");
          const data = Object.defineProperty({}, "value", {
            enumerable: true,
            get() {
              throw sentinel;
            },
          });
          let preserved = false;
          try {
            history.pushState(data, "", "/getter-failed");
          } catch (error) {
            preserved = error === sentinel;
          }
          return {
            sharedValue: new Uint8Array(sharedClone)[0],
            moduleClone: moduleClone instanceof WebAssembly.Module,
            errors,
            preserved,
            path: location.pathname,
            sameState: history.state === savedState,
            state: history.state,
            length: history.length,
          };
        }),
        {
          sharedValue: 37,
          moduleClone: true,
          errors: Array.from({ length: 4 }, () => ({ name: "DataCloneError", local: true })),
          preserved: true,
          path: "/first",
          sameState: true,
          state: { n: 1 },
          length: 3,
        },
      );
      assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
      assert.equal(new URL(page.url()).pathname, "/second");
      assert.deepEqual(await page.evaluate(() => history.state), { n: 2 });
      assert.deepEqual(requests, ["/host", "/one"]);
    },
    { isolated: true },
  );
});

test("nested same-document changes emit ordered frame notifications even for identical URLs", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    const frames = [];
    page.on("framenavigated", (frame) => {
      assert.equal(frame, page.mainFrame());
      frames.push(new URL(frame.url()).pathname);
    });
    await page.evaluate(() => {
      navigation.addEventListener("currententrychange", function nested() {
        navigation.removeEventListener("currententrychange", nested);
        history.replaceState({ n: 2 }, "", "/second");
        history.pushState({ n: 3 }, "", "/second");
      });
      history.pushState({ n: 1 }, "", "/first");
    });
    assert.deepEqual(frames, ["/first", "/second", "/second"]);
    assert.deepEqual(await page.evaluate(() => history.state), { n: 3 });
    await assertSameBack(page, "/second");
    assert.deepEqual(await page.evaluate(() => history.state), { n: 2 });
    await assertSameBack(page, "/one");
    assert.deepEqual(frames, ["/first", "/second", "/second", "/second", "/one"]);
    assert.deepEqual(requests, ["/host", "/one"]);
  });
});

test("Navigation entries index and traverseTo resist poisoned Array lookup and species hooks", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => {
      history.pushState({ n: 1 }, "", "/same");
      history.pushState({ n: 2 }, "", "/same");
    });
    assert.deepEqual(
      await page.evaluate(async () => {
        const define = Object.defineProperty;
        const descriptor = Object.getOwnPropertyDescriptor;
        const array = Array;
        const prototype = Array.prototype;
        const saved = {
          indexOf: descriptor(prototype, "indexOf"),
          map: descriptor(prototype, "map"),
          findIndex: descriptor(prototype, "findIndex"),
          constructor: descriptor(prototype, "constructor"),
          species: descriptor(array, Symbol.species),
        };
        let hooks = 0;
        const poisoned = () => {
          hooks++;
          throw new Error("guest Array hook must not run");
        };
        try {
          define(prototype, "indexOf", { configurable: true, writable: true, value: poisoned });
          define(prototype, "map", { configurable: true, writable: true, value: poisoned });
          define(prototype, "findIndex", { configurable: true, writable: true, value: poisoned });
          define(prototype, "constructor", { configurable: true, get: poisoned });
          define(array, Symbol.species, { configurable: true, get: poisoned });
          const entries = navigation.entries();
          const indices = [];
          for (let index = 0; index < entries.length; index++)
            indices[index] = entries[index].index;
          const distinct = entries[1].key !== entries[2].key && entries[1].id !== entries[2].id;
          const target = entries[1].key;
          await navigation.traverseTo(target).finished;
          const first = {
            path: location.pathname,
            index: navigation.currentEntry.index,
            state: history.state.n,
          };
          await navigation.traverseTo(entries[2].key).finished;
          return {
            length: entries.length,
            indices,
            distinct,
            first,
            last: {
              path: location.pathname,
              index: navigation.currentEntry.index,
              state: history.state.n,
            },
            hooks,
          };
        } finally {
          define(prototype, "indexOf", saved.indexOf);
          define(prototype, "map", saved.map);
          define(prototype, "findIndex", saved.findIndex);
          define(prototype, "constructor", saved.constructor);
          define(array, Symbol.species, saved.species);
        }
      }),
      {
        length: 3,
        indices: [0, 1, 2],
        distinct: true,
        first: { path: "/same", index: 1, state: 1 },
        last: { path: "/same", index: 2, state: 2 },
        hooks: 0,
      },
    );
    assert.deepEqual(await page.evaluate(() => popEvents), ["/same", "/same"]);
    assert.deepEqual(requests, ["/host", "/one"]);
  });
});

test("explicit undefined and null history states remain distinct across document-group restoration", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => history.pushState(undefined, "", "/undefined"));
    assert.equal(await page.evaluate(() => history.state), undefined);
    await page.evaluate(() => history.pushState(null, "", "/null"));
    assert.equal(await page.evaluate(() => history.state), null);
    await assign(page, "/real");
    await page.goBack({ waitUntil: "load", timeout: 5000 });
    assert.equal(new URL(page.url()).pathname, "/null");
    assert.equal(await page.evaluate(() => history.state), null);
    await page.evaluate(() => {
      window.restoredMarker = marker;
    });
    await assertSameBack(page, "/undefined");
    assert.equal(await page.evaluate(() => history.state), undefined);
    assert.equal(await page.evaluate(() => marker === restoredMarker), true);
    await assertSameBack(page, "/one");
    assert.equal(await page.evaluate(() => history.state), null);
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(new URL(page.url()).pathname, "/undefined");
    assert.equal(await page.evaluate(() => history.state), undefined);
    assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(new URL(page.url()).pathname, "/null");
    assert.equal(await page.evaluate(() => history.state), null);
    assert.deepEqual(requests, ["/host", "/one", "/real", "/null"]);
  });
});

test("popstate retains the selected state when currententrychange mutates history", async () => {
  await historyFixture(async ({ root, requests }) => {
    const page = await popup(root);
    await page.evaluate(() => {
      history.replaceState({ selected: true }, "", "/selected");
      history.pushState({ later: true }, "", "/later");
      window.observedPopState = null;
      addEventListener(
        "popstate",
        (event) => {
          window.observedPopState = event.state;
        },
        { once: true },
      );
      navigation.addEventListener("currententrychange", function change() {
        navigation.removeEventListener("currententrychange", change);
        history.pushState({ nested: true }, "", "/nested");
      });
    });
    assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
    assert.deepEqual(
      await page.evaluate(() => ({
        selected: observedPopState,
        current: history.state,
        path: location.pathname,
      })),
      {
        selected: { selected: true },
        current: { nested: true },
        path: "/nested",
      },
    );
    assert.deepEqual(requests, ["/host", "/one"]);
  });
});

test("initial empty history remains non-traversable after pushState before its first document navigation", async () => {
  await historyFixture(async ({ root, base, requests }) => {
    const page = await popup(root, "about:blank");
    assert.deepEqual(
      await page.evaluate(() => ({ length: history.length, entries: navigation.entries().length })),
      { length: 0, entries: 0 },
    );
    await page.evaluate(() => history.pushState({ pushed: true }, "", "about:blank"));
    assert.deepEqual(
      await page.evaluate(() => ({
        length: history.length,
        state: history.state,
        entries: navigation.entries().length,
      })),
      { length: 0, state: { pushed: true }, entries: 0 },
    );
    await assign(page, `${base}/real`);
    assert.equal(await page.evaluate(() => history.length), 1);
    assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
    assert.equal(page.url(), `${base}/real`);
    assert.equal(await page.evaluate(() => history.state), null);
    assert.deepEqual(requests, ["/host", "/real"]);
  });
});

test(
  "late request-context snapshots cannot replace an authoritative native history URL",
  { timeout: 10000 },
  async () => {
    await historyFixture(async ({ root, base, requests }) => {
      const page = await popup(root);
      let captured;
      let release;
      const capturedSnapshot = new Promise((resolve) => {
        captured = resolve;
      });
      const released = new Promise((resolve) => {
        release = resolve;
      });
      const collect = page._collectSession;
      let held = false;
      page._collectSession = async function (...args) {
        const result = await collect.apply(this, args);
        if (!held && args[1].includes("requestFrameSnapshot")) {
          held = true;
          captured();
          await released;
        }
        return result;
      };
      const olderRefresh = page._refreshFromSession();
      olderRefresh.catch(() => {});
      try {
        await capturedSnapshot;
        await assign(page, "/two");
        await page.waitForURL(`${base}/two`, { timeout: 5000 });
        await page.evaluate(() => {
          window.currentMarker = marker;
        });
        release();
        await olderRefresh;
        assert.equal(page.url(), `${base}/two`);
        assert.equal(await page.evaluate(() => location.pathname), "/two");
        assert.equal(await page.evaluate(() => marker === currentMarker), true);
        assert.deepEqual(requests, ["/host", "/one", "/two"]);
      } finally {
        release();
        page._collectSession = collect;
        await olderRefresh.catch(() => {});
      }
    });
  },
);
