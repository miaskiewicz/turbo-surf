(async () => {
  // Wait for the page's used @font-face faces before a geometry probe reads it.
  await document.fonts.ready;
  return JSON.stringify(Array.from(document.fonts, (face) => [face.family, face.status]));
})();
