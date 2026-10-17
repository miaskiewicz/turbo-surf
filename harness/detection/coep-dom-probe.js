(() => {
  const sheet = document.getElementById("sheet").sheet;
  let rules = null,
    rulesError = null;
  try {
    rules = Array.from(sheet.cssRules, (rule) => rule.selectorText);
  } catch (error) {
    rulesError = error.name;
  }
  const image = document.getElementById("image");
  return {
    events: resourceEvents.slice().sort(),
    scripts: { plain: globalThis.coepPlain || 0, authorized: globalThis.coepAuthorized || 0 },
    sheet: { present: sheet !== null, rules, rulesError },
    integritySheet: document.getElementById("integrity").sheet === null,
    color: getComputedStyle(document.getElementById("sentinel")).color,
    image: { complete: image.complete, width: image.naturalWidth, height: image.naturalHeight },
  };
})();
