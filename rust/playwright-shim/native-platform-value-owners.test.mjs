import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let BrowserContext;
try {
  ({ BrowserContext } = await import("./index.mjs"));
} catch {
  // The offline suite also runs before the native addon has been built.
}

async function withPage(run) {
  const server = createServer((_request, response) => {
    response.end('<body><iframe id="child" srcdoc="<body>child</body>"></iframe></body>');
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

for (const kind of ["DOMRect", "DOMPoint", "AudioBuffer"])
  for (const borrowed of [false, true])
    test(
      `${kind} retains its native guest owner and stream identity${borrowed ? " when borrowed" : ""}`,
      { timeout: 10000, skip: !BrowserContext },
      async () => {
        await withPage(async (page) => {
          const actual = await page.evaluate(
            async ({ kind, borrowed }) => {
              const realm = borrowed ? document.querySelector("#child").contentWindow : globalThis;
              const value =
                kind === "AudioBuffer"
                  ? new realm.AudioBuffer({ length: 8, numberOfChannels: 1, sampleRate: 48000 })
                  : new realm[kind](1, 2, 3, 4);
              const stream = new ReadableStream({
                start(controller) {
                  controller.enqueue(value);
                  controller.close();
                },
              });
              const read = await stream.getReader().read();
              return {
                same: read.value === value,
                prototype: Object.getPrototypeOf(value) === realm[kind].prototype,
                functionOwner: value.constructor.constructor("return globalThis")() === realm,
              };
            },
            { kind, borrowed },
          );
          assert.deepEqual(actual, { same: true, prototype: true, functionOwner: true });
        });
      },
    );

test(
  "borrowed native constructors preserve a guest-selected subclass prototype",
  { timeout: 10000, skip: !BrowserContext },
  async () => {
    await withPage(async (page) => {
      const actual = await page.evaluate(async () => {
        const child = document.querySelector("#child").contentWindow;
        class CustomRect extends child.DOMRect {}
        class CustomAudio extends child.AudioBuffer {}
        const values = [
          new CustomRect(1, 2, 3, 4),
          new CustomAudio({ length: 8, numberOfChannels: 1, sampleRate: 48000 }),
        ];
        const prototypes = [CustomRect.prototype, CustomAudio.prototype];
        const results = [];
        for (let index = 0; index < values.length; index++) {
          const value = values[index];
          const stream = new ReadableStream({
            start(controller) {
              controller.enqueue(value);
              controller.close();
            },
          });
          const read = await stream.getReader().read();
          results.push(read.value === value && Object.getPrototypeOf(value) === prototypes[index]);
        }
        return results;
      });
      assert.deepEqual(actual, [true, true]);
    });
  },
);
