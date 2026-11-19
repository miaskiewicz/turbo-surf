import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let newPage, BrowserContext;
try {
  ({ newPage, BrowserContext } = await import("../index.mjs"));
} catch {
  // The offline suite also runs before the native addon has been built.
}

test(
  "private infrastructure keeps streams alive and retires guest callbacks",
  { timeout: 10000, skip: !newPage },
  async () => {
    let releaseBody;
    const wire = [];
    const server = createServer((req, res) => {
      wire.push(req.url);
      res.setHeader("Connection", "close");
      if (req.url === "/stream") {
        res.setHeader("Content-Type", "text/plain");
        res.write("first");
        releaseBody = () => res.end("second");
      } else {
        res.setHeader("Content-Type", "text/html");
        res.end("<body>lifetime diagnostic</body>");
      }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const base = `http://127.0.0.1:${server.address().port}`;
    const page = newPage();
    let child;
    const result = { prototype: true, normalProxyReuse: true, wire };
    const pause = () => new Promise((resolve) => setTimeout(resolve, 10));
    try {
      await page.goto(`${base}/first`, { waitUntil: "networkidle" });
      [child] = await Promise.all([
        page.waitForEvent("popup", { timeout: 2000 }),
        page.evaluate(() => !!window.open("about:blank", "keeper")),
      ]);
      await page.evaluate(() => {
        const keeper = window.open("", "keeper");
        const cell = { ran: false };
        keeper.oldGuestCell = cell;
        new Promise((resolve) => {
          keeper.resolveOldGuest = resolve;
        }).then(() => {
          cell.ran = true;
        });
        const bytes = new ReadableStream({
          type: "bytes",
          pull(controller) {
            keeper.retiredRequest = controller.byobRequest;
          },
        });
        bytes.getReader({ mode: "byob" }).read(new Uint8Array(4));
      });
      await child.evaluate(() => {
        globalThis.savedOpener = opener;
      });
      assert.equal(await child.evaluate(() => typeof retiredRequest), "object");
      result.targetBefore = page._targetId;
      await page.goto(`${base}/next`, { waitUntil: "networkidle" });
      result.targetAfter = page._targetId;
      await child.evaluate(() => {
        resolveOldGuest("done");
      });
      result.retiredFrontendDenied = await child.evaluate(() => {
        try {
          const view = retiredRequest.view;
          return view === null;
        } catch (error) {
          return error.name === "TypeError";
        }
      });
      assert.equal(result.retiredFrontendDenied, true);
      await page.evaluate(() => {
        globalThis.lifetimePhases = [];
        fetch("/stream")
          .then(async (response) => {
            lifetimePhases.push("headers");
            const reader = response.body.getReader();
            for (;;) {
              const part = await reader.read();
              if (part.done) {
                lifetimePhases.push("eof");
                break;
              }
              const value = part.value;
              if (
                !(value instanceof Uint8Array) ||
                !(value.buffer instanceof ArrayBuffer) ||
                Object.getPrototypeOf(value) !== Uint8Array.prototype ||
                value.constructor.constructor("return globalThis")() !== globalThis
              )
                throw new Error("stream chunk escaped the guest realm");
              lifetimePhases.push(`chunk:${new TextDecoder().decode(value)}`);
            }
          })
          .catch((error) => lifetimePhases.push(`error:${error.name}:${error.message}`));
      });
      const deadline = Date.now() + 2000;
      while (Date.now() < deadline) {
        result.phases = JSON.parse(
          await page._readEngine("return JSON.stringify(globalThis.lifetimePhases);"),
        );
        if (result.phases.includes("chunk:first")) break;
        await pause();
      }
      result.firstChunkBeforeRelease = result.phases.includes("chunk:first");
      releaseBody?.();
      const eofDeadline = Date.now() + 2000;
      while (Date.now() < eofDeadline) {
        result.phases = JSON.parse(
          await page._readEngine("return JSON.stringify(globalThis.lifetimePhases);"),
        );
        if (result.phases.includes("eof")) break;
        await pause();
      }
      result.oldGuestRan = JSON.parse(
        await child._readEngine("return JSON.stringify(oldGuestCell.ran);"),
      );
      result.bodyRecovered =
        result.firstChunkBeforeRelease &&
        result.phases.includes("chunk:second") &&
        result.phases.includes("eof");
      result.openerIdentityPreserved = await child.evaluate(
        () => opener === savedOpener && savedOpener.location.pathname === "/next",
      );
      assert.equal(result.oldGuestRan, false);
      assert.equal(result.targetBefore, result.targetAfter);
      assert.equal(result.openerIdentityPreserved, true);
      assert.equal(result.bodyRecovered, true);
      console.log(JSON.stringify(result, null, 2));
    } finally {
      releaseBody?.();
      await page.context().close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

async function withBoundaryPage(run) {
  const server = createServer((request, response) => {
    if (request.url === "/delayed") setTimeout(() => response.end("after-root-close"), 25);
    else response.end("<body>boundary fixture</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new BrowserContext();
  const page = await context.newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await run(page);
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test(
  "guest stream values preserve identity and BYOB aliasing and detachment",
  { timeout: 10000, skip: !newPage },
  async () => {
    await withBoundaryPage(async (page) => {
      const actual = await page.evaluate(async () => {
        const object = { marker: 1 },
          guestError = new Error("guest-identity");
        const ordinary = new ReadableStream({
          start(controller) {
            controller.enqueue(object);
            controller.close();
          },
        });
        const sameObject = (await ordinary.getReader().read()).value === object;
        const failed = new ReadableStream({
          start(controller) {
            controller.error(guestError);
          },
        });
        let sameError = false;
        try {
          await failed.getReader().read();
        } catch (error) {
          sameError = error === guestError;
        }
        const bytes = new ReadableStream({
          type: "bytes",
          pull(controller) {
            controller.byobRequest.view[0] = 41;
            controller.byobRequest.respond(1);
            controller.close();
          },
        });
        const input = new Uint8Array(8),
          original = input.buffer;
        const part = await bytes.getReader({ mode: "byob" }).read(input);
        const byob =
          part.value instanceof Uint8Array &&
          part.value.buffer instanceof ArrayBuffer &&
          original.byteLength === 0 &&
          part.value[0] === 41;
        const alias = new Uint8Array(part.value.buffer);
        alias[0] = 73;
        const aliases = part.value[0] === 73;
        const transferred = structuredClone(part.value.buffer, { transfer: [part.value.buffer] });
        const detached = part.value.byteLength === 0 && new Uint8Array(transferred)[0] === 73;
        const constructorOwned =
          transferred.constructor.constructor("return globalThis")() === globalThis;

        return { sameObject, sameError, byob, aliases, detached, constructorOwned };
      });
      assert.deepEqual(actual, {
        sameObject: true,
        sameError: true,
        byob: true,
        aliases: true,
        detached: true,
        constructorOwned: true,
      });
    });
  },
);

test(
  "WebCrypto completion preserves the actual guest boundary",
  { timeout: 10000, skip: !newPage },
  async () => {
    await withBoundaryPage(async (page) => {
      const actual = await page.evaluate(async () => {
        const promiseThen = Object.getOwnPropertyDescriptor(Promise.prototype, "then");
        const promiseConstructor = Object.getOwnPropertyDescriptor(
          Promise.prototype,
          "constructor",
        );
        let hooks = 0,
          pendingDigest;
        try {
          // oxlint-disable-next-line unicorn/no-thenable -- Deliberately poison an existing Promise hook for the isolation control.
          Object.defineProperty(Promise.prototype, "then", {
            configurable: true,
            get() {
              hooks++;
              throw Error("private then exposed");
            },
          });
          Object.defineProperty(Promise.prototype, "constructor", {
            configurable: true,
            get() {
              hooks++;
              throw Error("private species exposed");
            },
          });
          pendingDigest = crypto.subtle.digest("SHA-256", new Uint8Array([1, 2]));
        } finally {
          // oxlint-disable-next-line unicorn/no-thenable -- Restore the native hook after the isolation control.
          Object.defineProperty(Promise.prototype, "then", promiseThen);
          Object.defineProperty(Promise.prototype, "constructor", promiseConstructor);
        }
        const digest = await pendingDigest;
        const privatePromiseHooks = hooks === 0;
        const cryptoOwned =
          digest instanceof ArrayBuffer &&
          digest.constructor.constructor("return globalThis")() === globalThis;
        const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, false, [
          "encrypt",
        ]);
        const keyStream = new ReadableStream({
          start(controller) {
            controller.error(key);
          },
        });
        let sameCryptoKey = false;
        try {
          await keyStream.getReader().read();
        } catch (error) {
          sameCryptoKey = error === key;
        }
        let cryptoErrorOwned = false;
        try {
          await crypto.subtle.digest("not-a-native-algorithm", new Uint8Array([1]));
        } catch (error) {
          cryptoErrorOwned =
            error instanceof DOMException &&
            error.constructor.constructor("return globalThis")() === globalThis;
        }

        return { cryptoOwned, privatePromiseHooks, sameCryptoKey, cryptoErrorOwned };
      });
      assert.deepEqual(actual, {
        cryptoOwned: true,
        privatePromiseHooks: true,
        sameCryptoKey: true,
        cryptoErrorOwned: true,
      });
    });
  },
);

test(
  "geometry arrays and matrix buffers belong to the actual guest",
  { timeout: 10000, skip: !newPage },
  async () => {
    await withBoundaryPage(async (page) => {
      const actual = await page.evaluate(async () => {
        const elements = document.elementsFromPoint(0, 0),
          matrix = new DOMMatrix().toFloat64Array();
        const geometryOwned =
          Object.getPrototypeOf(elements) === Array.prototype &&
          elements.constructor.constructor("return globalThis")() === globalThis &&
          matrix instanceof Float64Array &&
          matrix.buffer instanceof ArrayBuffer &&
          matrix.constructor.constructor("return globalThis")() === globalThis;

        return { geometryOwned };
      });
      assert.deepEqual(actual, { geometryOwned: true });
    });
  },
);

test(
  "compression constructors roundtrip bytes without exposing the private realm",
  { timeout: 10000, skip: !newPage },
  async () => {
    await withBoundaryPage(async (page) => {
      const actual = await page.evaluate(async () => {
        const owned = (value) =>
          value.constructor.constructor("return globalThis")() === globalThis;
        const constructors = [CompressionStream, DecompressionStream];
        const streams = constructors.map((constructor) => Reflect.construct(constructor, ["gzip"]));
        const brands = streams.map(
          (stream, index) =>
            stream instanceof constructors[index] &&
            stream.readable instanceof ReadableStream &&
            stream.writable instanceof WritableStream &&
            owned(stream) &&
            owned(stream.readable) &&
            owned(stream.writable),
        );
        const formatErrors = constructors.map((constructor) => {
          try {
            Reflect.construct(constructor, ["unsupported"]);
            return false;
          } catch (error) {
            return error instanceof TypeError && owned(error);
          }
        });
        const receiverErrors = constructors.map((constructor) => {
          try {
            Object.getOwnPropertyDescriptor(constructor.prototype, "readable").get.call({});
            return false;
          } catch (error) {
            return error instanceof TypeError && owned(error);
          }
        });
        const decoded = new Blob([new Uint8Array([4, 5, 6])])
          .stream()
          .pipeThrough(streams[0])
          .pipeThrough(streams[1]);
        const pending = new Response(decoded).arrayBuffer();
        const promiseOwned = pending instanceof Promise && owned(pending);
        const bytes = await pending;
        return {
          brands,
          formatErrors,
          receiverErrors,
          promiseOwned,
          bufferOwned: bytes instanceof ArrayBuffer && owned(bytes),
          bytes: Array.from(new Uint8Array(bytes)),
          privateCore: typeof Deno,
        };
      });
      assert.deepEqual(actual, {
        brands: [true, true],
        formatErrors: [true, true],
        receiverErrors: [true, true],
        promiseOwned: true,
        bufferOwned: true,
        bytes: [4, 5, 6],
        privateCore: "undefined",
      });
    });
  },
);
test(
  "a surviving child completes new delayed IO after its root closes",
  { timeout: 10000, skip: !newPage },
  async () => {
    await withBoundaryPage(async (page) => {
      const [child] = await Promise.all([
        page.waitForEvent("popup", { timeout: 2000 }),
        page.evaluate(() => !!window.open("about:blank", "survivor")),
      ]);
      await child.evaluate(() => {
        globalThis.survivalMarker = "same-native-child";
      });
      const target = child._targetId;
      await page.close();
      assert.equal(child.isClosed(), false);
      assert.equal(await child.evaluate(() => survivalMarker), "same-native-child");
      assert.equal(child._targetId, target);
      assert.equal(
        await child.evaluate(async () => await (await fetch("/delayed")).text()),
        "after-root-close",
      );
    });
  },
);

test(
  "closed owned contexts reject page evaluation without reopening native roots",
  { timeout: 10000, skip: !newPage },
  async () => {
    const server = createServer((request, response) => response.end("<body>close fixture</body>"));
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const page = newPage();
    try {
      await page.goto(`http://127.0.0.1:${server.address().port}/`);
      const [child] = await Promise.all([
        page.waitForEvent("popup", { timeout: 2000 }),
        page.evaluate(() => !!window.open("about:blank", "closed-child")),
      ]);
      await page.close();
      assert.equal(page.isClosed(), true);
      assert.equal(child.isClosed(), true);
      await assert.rejects(
        page.evaluate(() => 1),
        /closed/,
      );
      await assert.rejects(
        child.evaluate(() => 1),
        /closed/,
      );
    } finally {
      await page.context().close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
