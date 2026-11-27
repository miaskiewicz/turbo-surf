import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

// Compile rather than vendor webpack's MIT-licensed generated runtime. Accept a
// standard webpack installation or Next's bundled compiler via this explicit path.
const require = createRequire(import.meta.url);
let webpack;
try {
  const compiler = require(process.env.TURBO_SURF_WEBPACK_COMPILER || "webpack");
  compiler.init?.();
  webpack = compiler.webpack || compiler;
} catch (error) {
  if (process.env.TURBO_SURF_WEBPACK_COMPILER) throw error;
}

const sources = {
  "second.js":
    "import { base } from './shared.js';\nimport { suffix } from './lazy-shared.js';\nexport const value = () => base + '-' + suffix + '-second';\n",
  "lazy-shared.js":
    "globalThis.lazyExecutions = (globalThis.lazyExecutions || 0) + 1;\nexport const suffix = 'split';\n",
  "first.js":
    "import { base } from './shared.js';\nimport { suffix } from './lazy-shared.js';\nexport const value = () => base + '-' + suffix + '-first';\n",
  "retry.js": "export const value = 'retried-real-chunk';\n",
  "shared.js":
    "globalThis.sharedExecutions = (globalThis.sharedExecutions || 0) + 1;\nexport const base = 'webpack';\n",
  "entry.js":
    "import { base } from './shared.js';\nglobalThis.initialValue = base;\nglobalThis.loadFirst = () => import(/* webpackChunkName: \"first\" */ './first.js').then(m => m.value());\nglobalThis.loadSecond = () => import(/* webpackChunkName: \"second\" */ './second.js').then(m => m.value());\nglobalThis.loadRetry = () => import(/* webpackChunkName: \"retry\" */ './retry.js').then(m => m.value);\ndocument.querySelector('#load').addEventListener('click', async () => {\n  document.querySelector('#result').textContent = await globalThis.loadFirst();\n});\n",
};

test(
  "genuine webpack production runtime loads shared split chunks and retries failures",
  { skip: !webpack && "webpack compiler unavailable", timeout: 15000 },
  async () => {
    const directory = await mkdtemp(join(tmpdir(), "surf-webpack-"));
    try {
      for (const [name, source] of Object.entries(sources))
        await writeFile(join(directory, name), source);
      const stats = await new Promise((resolve, reject) =>
        webpack(
          {
            mode: "production",
            context: directory,
            entry: { main: "./entry.js" },
            output: {
              path: join(directory, "dist"),
              filename: "[name].[contenthash].js",
              chunkFilename: "[name].[contenthash].js",
              publicPath: "auto",
            },
            optimization: {
              minimize: false,
              runtimeChunk: "single",
              splitChunks: { chunks: "all", minSize: 0 },
            },
          },
          (error, result) =>
            error
              ? reject(error)
              : result.hasErrors()
                ? reject(new Error(result.toString()))
                : resolve(result.toJson({ all: false, assets: true, entrypoints: true })),
        ),
      );
      const assets = new Map(
        await Promise.all(
          stats.assets
            .filter((a) => a.name.endsWith(".js"))
            .map(async (a) => [a.name, await readFile(join(directory, "dist", a.name))]),
        ),
      );
      const initial = stats.entrypoints.main.assets.map((a) => a.name);
      const fetched = [];
      let failRetry = true;
      const html =
        '<body><button id="load">Load</button><div id="result">pending</div>' +
        initial.map((n) => `<script defer src="/assets/${n}"></script>`).join("") +
        "</body>";
      const server = createServer((req, res) => {
        fetched.push(req.url);
        const name = req.url.replace("/assets/", "");
        if (assets.has(name)) {
          if (name.startsWith("retry.") && failRetry) {
            failRetry = false;
            res.writeHead(404);
            res.end("missing fixture chunk");
            return;
          }
          res.writeHead(200, { "content-type": "application/javascript" });
          res.end(assets.get(name));
          return;
        }
        res.writeHead(200, { "content-type": "text/html" });
        res.end(html);
      });
      await new Promise((r) => server.listen(0, "127.0.0.1", r));
      const context = new shim.BrowserContext();
      try {
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (e) => errors.push(e.message));
        const url = `http://127.0.0.1:${server.address().port}`;
        await page.goto(url, { waitUntil: "load", timeout: 10000 });
        assert.equal(await page.evaluate(() => initialValue), "webpack");
        assert.equal(fetched.filter((x) => x.includes("/assets/")).length, 2);
        await page.locator("#load").click({ timeout: 3000 });
        await page.locator("#result").waitFor({ state: "visible", timeout: 3000 });
        assert.equal(await page.locator("#result").textContent(), "webpack-split-first");
        assert.deepEqual(
          await page.evaluate(async () => Promise.all([loadSecond(), loadSecond(), loadFirst()])),
          ["webpack-split-second", "webpack-split-second", "webpack-split-first"],
        );
        assert.equal(await page.evaluate(() => sharedExecutions), 1);
        assert.equal(await page.evaluate(() => lazyExecutions), 1);
        const once = [...assets.keys()].filter((n) => !n.startsWith("retry."));
        for (const name of once)
          assert.equal(fetched.filter((p) => p === "/assets/" + name).length, 1, name);

        const failure = await page.evaluate(async () => {
          try {
            await loadRetry();
            return null;
          } catch (e) {
            return { name: e.name, type: e.type, request: e.request };
          }
        });
        assert.equal(failure.name, "ChunkLoadError");
        assert.equal(failure.type, "error");
        assert.match(failure.request, /\/assets\/retry\..*\.js$/);
        assert.equal(await page.evaluate(() => loadRetry()), "retried-real-chunk");
        assert.equal(errors.length, 0);
      } finally {
        await context.close();
        server.closeAllConnections();
        await new Promise((r) => server.close(r));
      }
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  },
);
