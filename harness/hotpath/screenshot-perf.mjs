import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";
import { newPage } from "../../rust/playwright-shim/index.mjs";
import { pngPixels } from "./png-pixels.mjs";
import { withScreenshotImageSite, screenshotRowDocument } from "./screenshot-perf-fixture.mjs";

const native = createRequire(import.meta.url)("../../rust/crates/turbo-surf-napi/index.js");
const median = (values) =>
  [...values].sort((left, right) => left - right)[Math.floor(values.length / 2)];

export async function runScreenshotResourceBenchmark(captures = 5) {
  const results = [];
  await withScreenshotImageSite(native, async ({ url, requests }) => {
    let reference;
    for (const stage of ["all-image-control", "capture-resource-plan"]) {
      const page = newPage({ viewport: { width: 300, height: 200 } });
      try {
        await page.goto(url, { waitUntil: "networkidle" });
        const fetchImages = page._fetchImages;
        if (stage === "all-image-control") page._fetchImages = () => fetchImages.call(page);
        requests.length = 0;
        const timings = [];
        for (let capture = 0; capture < captures; capture++) {
          // A cold-resident control isolates which new pixel bodies a capture obtains.
          page._imageCache = undefined;
          const start = performance.now();
          const png = await page.screenshot({ externalCss: false });
          timings.push(performance.now() - start);
          reference ??= png;
          assert.ok(png.equals(reference));
          const pixels = pngPixels(png);
          assert.equal(pixels.width, 300);
          assert.equal(pixels.height, 200);
          assert.deepEqual(pixels.pixel(150, 100).slice(0, 3), [255, 0, 0]);
        }
        assert.equal(await page.locator("img").count(), 6);
        const expectedRequests = captures * (stage === "all-image-control" ? 6 : 1);
        assert.equal(requests.length, expectedRequests);
        results.push({
          stage,
          captures,
          imageHTTPRequests: requests.length,
          medianMs: median(timings),
          ...page._captureResourceStats,
          byteIdentical: true,
        });
      } finally {
        await page.close();
      }
    }
  });
  return results;
}

export async function runScreenshotSceneBenchmark(captures = 6) {
  const results = [];
  for (const documentRows of [20, 500, 5000]) {
    const html = screenshotRowDocument(documentRows);
    let reference;
    for (const stage of ["stateless-prepare-per-capture", "retained-scene"]) {
      const page = newPage({ viewport: { width: 300, height: 200 } });
      try {
        await page.setContent(html);
        let preparations = 0;
        const timings = [];
        for (let capture = 0; capture < captures; capture++) {
          const start = performance.now();
          let png;
          if (stage === "stateless-prepare-per-capture") {
            png = native.screenshotCapture(html, "", {}, 300, 200, "{}", "{}");
            preparations++;
          } else png = await page.screenshot({ images: false, externalCss: false });
          timings.push(performance.now() - start);
          reference ??= png;
          assert.ok(png.equals(reference));
          const pixels = pngPixels(png);
          assert.equal(pixels.width, 300);
          assert.equal(pixels.height, 200);
          assert.deepEqual(pixels.pixel(5, 5).slice(0, 3), [255, 0, 0]);
          assert.deepEqual(pixels.pixel(5, 15).slice(0, 3), [0, 0, 255]);
        }
        preparations = page._screenshotSceneStats?.preparations ?? preparations;
        assert.equal(preparations, stage === "retained-scene" ? 1 : captures);
        assert.equal(await page.locator(".row").count(), documentRows);
        results.push({
          stage,
          documentRows,
          captures,
          preparations,
          medianWarmMs: median(timings.slice(1)),
          byteIdentical: true,
        });
      } finally {
        await page.close();
      }
    }
  }
  return results;
}

function artifactIdentity() {
  const path = process.env.TURBO_SURF_NAPI_PATH;
  const manifest = readFileSync(new URL("../../rust/Cargo.toml", import.meta.url), "utf8");
  return {
    manifestProducerPin: /rev = "([a-f0-9]{40})"/.exec(manifest)?.[1] ?? null,
    nativeVersion: native.version(),
    addonPath: path ?? null,
    addonSha256: path ? createHash("sha256").update(readFileSync(path)).digest("hex") : null,
  };
}

// `npm run harness:screenshots -- --output=/tmp/screenshots.json` saves a clean
// machine-readable report independently of node:test's incidental process summary.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const report = {
    identity: artifactIdentity(),
    resources: await runScreenshotResourceBenchmark(),
    scenes: await runScreenshotSceneBenchmark(),
  };
  const json = JSON.stringify(report, null, 2);
  const output = process.argv.find((argument) => argument.startsWith("--output="))?.slice(9);
  if (output) writeFileSync(output, `${json}\n`);
  console.log(json);
}
