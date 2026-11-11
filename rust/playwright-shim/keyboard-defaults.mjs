function isTextControl(element) {
  return (
    /^(INPUT|TEXTAREA)$/.test(element.tagName) &&
    !/^(checkbox|radio|button|submit|reset|file|image|hidden|range|color)$/.test(element.type)
  );
}
function deletionBounds(value, start, end, inputType) {
  if (start !== end) return [start, end];
  if (inputType === "deleteContentBackward")
    start = Math.max(0, start - (value.codePointAt(start - 2) > 0xffff ? 2 : 1));
  if (inputType === "deleteContentForward")
    end = Math.min(value.length, end + (value.codePointAt(end) > 0xffff ? 2 : 1));
  return [start, end];
}
function replaceKeyboardControl(element, text, inputType, replaceControlSelection) {
  const previous = element.value;
  // Native typing updates caret/value ranges without API select events.
  if (inputType === "insertText" && replaceControlSelection?.(element, text)) return true;
  const start = element.selectionStart ?? previous.length;
  const end = element.selectionEnd ?? start;
  const [from, to] = deletionBounds(previous, start, end, inputType);
  element.value = previous.slice(0, from) + text + previous.slice(to);
  if (element.value === previous) return false;
  if (element.selectionStart != null)
    element.setSelectionRange(from + text.length, from + text.length);
  return true;
}
function replaceKeyboardEditable(element, text) {
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
function editKeyboardText(element, text, inputType = "insertText", replaceControlSelection = null) {
  const control = isTextControl(element);
  if ((!control && !element.isContentEditable) || element.disabled || element.readOnly) return;
  const edit = {
    bubbles: true,
    composed: true,
    inputType,
    data: inputType === "insertText" ? text : null,
  };
  if (!element.dispatchEvent(new InputEvent("beforeinput", { ...edit, cancelable: true }))) return;
  if (control) {
    if (!replaceKeyboardControl(element, text, inputType, replaceControlSelection)) return;
  } else replaceKeyboardEditable(element, text);
  element.dispatchEvent(new InputEvent("input", edit));
}
function selectKeyboardText(element) {
  if (typeof element.select === "function") element.select();
  else if (element.isContentEditable) {
    const range = document.createRange();
    range.selectNodeContents(element);
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }
}
function activateKeyboardTarget(element, options, phase) {
  if (element.disabled) return;
  const input = element.tagName === "INPUT";
  const button =
    element.tagName === "BUTTON" || (input && /^(button|submit|reset)$/.test(element.type));
  const spaceControl = button || (input && /^(checkbox|radio)$/.test(element.type));
  const link = element.tagName === "A" && element.hasAttribute("href");
  const enter = phase === "keydown" && options.key === "Enter" && (button || link);
  const space = phase === "keyup" && options.key === " " && spaceControl;
  if (enter || space) element.click();
}
function isSelectAllShortcut(options) {
  return (
    (options.ctrlKey || options.metaKey) && options.key.toLowerCase() === "a" && !options.altKey
  );
}
function applyKeyboardDefault(element, options, phase, replaceControlSelection) {
  activateKeyboardTarget(element, options, phase);
  if (phase !== "keydown") return;
  if (isSelectAllShortcut(options)) {
    selectKeyboardText(element);
    return;
  }
  if (options.ctrlKey || options.metaKey || options.altKey) return;
  const deletion = { Backspace: "deleteContentBackward", Delete: "deleteContentForward" }[
    options.key
  ];
  if (deletion) editKeyboardText(element, "", deletion, replaceControlSelection);
  else if (Array.from(options.key).length === 1)
    editKeyboardText(element, options.key, "insertText", replaceControlSelection);
}

export function keyboardDefaultsSource() {
  return [
    isTextControl,
    deletionBounds,
    replaceKeyboardControl,
    replaceKeyboardEditable,
    editKeyboardText,
    selectKeyboardText,
    activateKeyboardTarget,
    isSelectAllShortcut,
    applyKeyboardDefault,
  ]
    .map((fn) => fn.toString())
    .join(";\n");
}
