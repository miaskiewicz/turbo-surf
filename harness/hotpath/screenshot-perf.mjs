import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";
import { newPage } from "../../rust/playwright-shim/index.mjs";
import { pngPixels } from "./png-pixels.mjs";
import { withScreenshotImageSite } from "./screenshot-perf-fixture.mjs";

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

function artifactIdentity() {
  const path = process.env.TURBO_SURF_NAPI_PATH;
  return {
    nativeVersion: native.version(),
    addonPath: path ?? null,
    addonSha256: path ? createHash("sha256").update(readFileSync(path)).digest("hex") : null,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(
    JSON.stringify(
      { identity: artifactIdentity(), resources: await runScreenshotResourceBenchmark() },
      null,
      2,
    ),
  );
}
