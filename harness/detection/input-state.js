// Type filtering identifies a query control, not its computed visibility.
(document, dismissSelector) => {
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
    controlTag: control?.tagName ?? null,
    controlType: control?.type ?? null,
    value: control?.value ?? null,
    activeTag: document.activeElement?.tagName ?? null,
    activeName: document.activeElement?.getAttribute("name") ?? null,
    disabled: control?.disabled ?? null,
    readOnly: control?.readOnly ?? null,
    consentPresent: !!document.querySelector(dismissSelector),
  };
};
