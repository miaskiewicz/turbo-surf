export function editKeyboardText(
  element,
  text,
  inputType = "insertText",
  replaceControlSelection = null,
) {
  const control = /^(INPUT|TEXTAREA)$/.test(element.tagName);
  if ((!control && !element.isContentEditable) || element.disabled || element.readOnly) return;
  if (
    control &&
    /^(checkbox|radio|button|submit|reset|file|image|hidden|range|color)$/.test(element.type)
  )
    return;
  if (
    !element.dispatchEvent(
      new InputEvent("beforeinput", {
        bubbles: true,
        composed: true,
        cancelable: true,
        inputType,
        data: inputType === "insertText" ? text : null,
      }),
    )
  )
    return;
  if (control) {
    const previous = element.value;
    let start = element.selectionStart ?? previous.length;
    let end = element.selectionEnd ?? start;
    if (start === end && inputType === "deleteContentBackward")
      start = Math.max(0, start - (previous.codePointAt(start - 2) > 0xffff ? 2 : 1));
    if (start === end && inputType === "deleteContentForward")
      end = Math.min(previous.length, end + (previous.codePointAt(end) > 0xffff ? 2 : 1));
    // Native typing updates caret/value ranges without API select events.
    const replaced = inputType === "insertText" && replaceControlSelection?.(element, text);
    if (!replaced) {
      element.value = previous.slice(0, start) + text + previous.slice(end);
      if (element.value === previous) return;
      if (element.selectionStart != null)
        element.setSelectionRange(start + text.length, start + text.length);
    }
  } else {
    const selection = getSelection();
    const range = selection.rangeCount ? selection.getRangeAt(0) : document.createRange();
    if (!element.contains(range.commonAncestorContainer)) {
      range.selectNodeContents(element);
      range.collapse(false);
    }
    range.deleteContents();
    const node = document.createTextNode(text);
    range.insertNode(node);
    range.setStartAfter(node);
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
  }
  element.dispatchEvent(
    new InputEvent("input", {
      bubbles: true,
      composed: true,
      inputType,
      data: inputType === "insertText" ? text : null,
    }),
  );
}

export function applyKeyboardDefault(element, options, phase, replaceControlSelection) {
  const button =
    element.tagName === "BUTTON" ||
    (element.tagName === "INPUT" && /^(button|submit|reset)$/.test(element.type));
  const spaceControl =
    button || (element.tagName === "INPUT" && /^(checkbox|radio)$/.test(element.type));
  const activate =
    !element.disabled && (button || (element.tagName === "A" && element.hasAttribute("href")));
  if (phase === "keyup") {
    if (options.key === " " && spaceControl && !element.disabled) element.click();
    return;
  }
  if (options.key === "Enter" && activate) {
    element.click();
    return;
  }
  if (
    (options.ctrlKey || options.metaKey) &&
    options.key.toLowerCase() === "a" &&
    !options.altKey
  ) {
    if (typeof element.select === "function") element.select();
    else if (element.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(element);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    return;
  }
  if (options.ctrlKey || options.metaKey || options.altKey) return;
  if (options.key === "Backspace")
    editKeyboardText(element, "", "deleteContentBackward", replaceControlSelection);
  else if (options.key === "Delete")
    editKeyboardText(element, "", "deleteContentForward", replaceControlSelection);
  else if (Array.from(options.key).length === 1)
    editKeyboardText(element, options.key, "insertText", replaceControlSelection);
}
