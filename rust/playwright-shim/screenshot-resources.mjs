function samePlan(previous, inputs) {
  return previous && previous.inputs.every((input, index) => input === inputs[index]);
}

export async function captureAssets(
  page,
  native,
  opts,
  width,
  height,
  optionsJson,
  html = page._html,
) {
  const [css, metadataJson] = await Promise.all([
    opts.externalCss === false ? "" : page._fetchLinkedCss(),
    page._session == null
      ? "{}"
      : page._evalEngine("globalThis.__RESULT = JSON.stringify(__tsNs.screenshotImageMetadata());"),
  ]);
  if (opts.images === false) return { css, images: {}, metadataJson };
  const inputs = [html, css, width, height, metadataJson, optionsJson];
  if (!samePlan(page._capturePlan, inputs)) {
    const plan = JSON.parse(
      native.screenshotCaptureResources(html, css, width, height, metadataJson, optionsJson),
    );
    page._capturePlan = { inputs, refs: plan.pixelUrls };
    const stats = (page._captureResourceStats ??= { plans: 0, fragmentsExamined: 0 });
    stats.plans++;
    stats.fragmentsExamined += plan.fragmentsExamined;
  }
  const images = await page._fetchImages(page._capturePlan.refs);
  return { css, images, metadataJson };
}
