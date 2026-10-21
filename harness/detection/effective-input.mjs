// Read-only evidence: never repair a partially delivered trusted input sequence.
export function effectiveTypedControl(control, expected) {
  return {
    expected,
    value: control?.value ?? null,
    focused: !!control && control.ownerDocument.activeElement === control,
    connected: control?.isConnected === true,
    editableControl: !!control && ["INPUT", "TEXTAREA"].includes(control.tagName),
    controlTag: control?.tagName ?? null,
    activeTag: control?.ownerDocument.activeElement?.tagName ?? null,
  };
}

export function prepareTypedControl(control, text) {
  if (
    !control ||
    !["INPUT", "TEXTAREA"].includes(control.tagName) ||
    control.ownerDocument.activeElement !== control ||
    !control.isConnected
  )
    throw new Error("Typing requires the intended connected editable control to remain focused");
  const value = control.value;
  const start = control.selectionStart ?? value.length;
  const end = control.selectionEnd ?? value.length;
  return {
    initialValue: value,
    selectionStart: start,
    selectionEnd: end,
    expected: value.slice(0, start) + text + value.slice(end),
  };
}

export function requireEffectiveInput(state) {
  if (
    !state.editableControl ||
    !state.connected ||
    !state.focused ||
    state.value !== state.expected
  ) {
    const error = new Error(
      "Trusted typing did not produce the expected focused query; Enter was not sent",
    );
    error.inputEvidence = state;
    throw error;
  }
  return state;
}

export function chromeExecutable(config, env = process.env) {
  return config.chromePath || env.TURBO_SURF_CHROME_PATH || null;
}
