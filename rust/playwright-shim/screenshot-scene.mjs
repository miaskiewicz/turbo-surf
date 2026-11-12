import { createHash } from "node:crypto";

function sceneKey(page, { html, css, images, width, height, optionsJson, metadataJson }) {
  const references = Object.keys(images).sort();
  const hash = createHash("sha256").update(
    JSON.stringify([page._url, html, css, width, height, optionsJson, metadataJson, references]),
  );
  for (const reference of references) {
    const bytes = images[reference];
    hash.update(JSON.stringify([reference, bytes.byteLength]));
    hash.update(bytes);
  }
  return hash.digest("hex");
}

export function closeScreenshotScene(page) {
  page._screenshotScene?.scene.close();
  page._screenshotScene = undefined;
  page._capturePlan = undefined;
}

export function paintScreenshotScene(page, native, inputs) {
  const {
    html = page._html,
    css,
    images,
    width,
    height,
    optionsJson,
    metadataJson,
    asSvg,
  } = inputs;
  const key = sceneKey(page, { ...inputs, html });
  const stats = (page._screenshotSceneStats ??= { preparations: 0, paints: 0 });
  if (page._screenshotScene?.key !== key) {
    const scene = new native.ScreenshotScene(
      html,
      css,
      images,
      width,
      height,
      optionsJson,
      metadataJson,
    );
    page._screenshotScene?.scene.close();
    page._screenshotScene = { key, scene };
    stats.preparations++;
  }
  const scene = page._screenshotScene.scene;
  const output = asSvg ? Buffer.from(scene.svg(), "utf8") : scene.png();
  stats.paints++;
  return output;
}
