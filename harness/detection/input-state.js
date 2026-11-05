// Type filtering identifies a query control, not its computed visibility.
(document, dismissSelector) => {
  const read = (node, key) => node?.[key] ?? null;
  const candidates = document.querySelectorAll('textarea[name="q"],input[name="q"]');
  let control = null;
  let excludedHiddenQueryControls = 0;
  for (const candidate of candidates) {
    if (candidate.tagName === "INPUT" && candidate.type === "hidden") {
      excludedHiddenQueryControls++;
    } else if (!control) {
      control = candidate;
    }
  }
  return {
    selection: "first-q-control-with-non-hidden-type",
    queryControlCount: candidates.length,
    excludedHiddenQueryControls,
    controlTag: read(control, "tagName"),
    controlType: read(control, "type"),
    value: read(control, "value"),
    activeTag: read(document.activeElement, "tagName"),
    activeName: document.activeElement?.getAttribute("name") ?? null,
    disabled: read(control, "disabled"),
    readOnly: read(control, "readOnly"),
    consentPresent: !!document.querySelector(dismissSelector),
  };
};
