// Called through a V8 context slot owned by PageSession, never a guest global.
// Constructors, DOM methods and host dispatch are captured before page execution.
(() => {
  const document = globalThis.document;
  const query = document.querySelector.bind(document);
  const focus = HTMLElement.prototype.focus;
  const elementGeometry = ops.op_live_element_geometry;
  const slice = String.prototype.slice;
  const apply = Reflect.apply;
  const activeElement = Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get;
  const connected = Object.getOwnPropertyDescriptor(Node.prototype, "isConnected").get;
  const requestSubmit = HTMLFormElement.prototype.requestSubmit;
  const controls = document.querySelectorAll.bind(document);
  const matches = Element.prototype.matches;
  const controlGetters = [HTMLInputElement, HTMLButtonElement].map((Constructor) =>
    Object.fromEntries(
      ["form", "type"].map((key) => [
        key,
        Object.getOwnPropertyDescriptor(Constructor.prototype, key).get,
      ]),
    ),
  );
  const events = Object.fromEntries(
    ["Event", "SubmitEvent", "MouseEvent", "PointerEvent", "KeyboardEvent", "InputEvent"].map(
      (name) => [name, globalThis[name]],
    ),
  );
  const dispatch = ops.op_dispatch_host_event;
  const activate = ops.op_host_user_activation;
  const canceled = Object.getOwnPropertyDescriptor(Event.prototype, "defaultPrevented").get;
  const controlTag = Object.getOwnPropertyDescriptor(Element.prototype, "tagName").get;
  const controlValues = Object.create(null);
  for (const [tag, Constructor] of [
    ["INPUT", HTMLInputElement],
    ["TEXTAREA", HTMLTextAreaElement],
  ]) {
    const prototype = Constructor.prototype;
    controlValues[tag] = {
      value: Object.getOwnPropertyDescriptor(prototype, "value"),
      disabled: Object.getOwnPropertyDescriptor(prototype, "disabled").get,
      readOnly: Object.getOwnPropertyDescriptor(prototype, "readOnly").get,
      selectionStart: Object.getOwnPropertyDescriptor(prototype, "selectionStart").get,
      selectionEnd: Object.getOwnPropertyDescriptor(prototype, "selectionEnd").get,
      setSelectionRange: prototype.setSelectionRange,
    };
  }
  function editingControl(target) {
    try {
      return controlValues[apply(controlTag, target, [])] || null;
    } catch {
      return null;
    }
  }
  // Optional embedder evidence retains the actual control across snapshots, never a guest global.
  let guardedInput = null;
  function inputGuard(request) {
    if (request === "clear") {
      guardedInput = null;
      return;
    }
    if (request && typeof request.begin === "string") {
      guardedInput = null;
      const target = request.selector
        ? query(request.selector)
        : apply(activeElement, document, []);
      const getters = editingControl(target);
      if (
        !getters ||
        apply(activeElement, document, []) !== target ||
        !apply(connected, target, [])
      )
        throw new Error(
          "Typing requires the intended connected editable control to remain focused",
        );
      const value = apply(getters.value.get, target, []);
      const start = apply(getters.selectionStart, target, []) ?? value.length;
      const end = apply(getters.selectionEnd, target, []) ?? value.length;
      guardedInput = {
        target,
        getters,
        expected: apply(slice, value, [0, start]) + request.begin + apply(slice, value, [end]),
      };
      return;
    }
    if (request !== "verify") throw new TypeError("Unsupported input guard request");
    if (!guardedInput) throw new Error("No retained input control to verify");
    const retained = guardedInput;
    guardedInput = null;
    const { target, getters, expected } = retained;
    const state = {
      expected,
      value: apply(getters.value.get, target, []),
      connected: apply(connected, target, []),
      focused: apply(activeElement, document, []) === target,
    };
    if (!state.connected || !state.focused || state.value !== expected) {
      throw new Error(
        "Trusted typing did not produce the expected focused query; Enter was not sent: " +
          JSON.stringify(state),
      );
    }
    guardedInput = retained;
  }
  const schedule = setTimeout.bind(globalThis),
    clear = clearTimeout.bind(globalThis);
  const waits = new Map();
  let activeRun = 0;
  const wait = (milliseconds) =>
    new Promise((resolve) => {
      const timer = schedule(() => {
        waits.delete(timer);
        resolve();
      }, milliseconds);
      waits.set(timer, resolve);
    });
  const cancel = () => {
    activeRun++;
    guardedInput = null;
    for (const [timer, resolve] of waits) {
      clear(timer);
      resolve();
    }
    waits.clear();
  };
  let hovered = null,
    cursorX = 0,
    cursorY = 0;
  function fire(target, name, type, init = {}) {
    const event = new events[name](type, init);
    // Only this captured embedder driver can notify activation. Dispatching an
    // identically named guest event never reaches this capability.
    if (
      (type === "pointerdown" && init.pointerType === "mouse") ||
      type === "mousedown" ||
      (type === "keydown" && init.key !== "Escape")
    )
      activate();
    dispatch(event, target);
    return !canceled.call(event);
  }
  function element(selector) {
    const result = query(selector);
    if (!result) throw new Error("Missing input selector " + selector);
    return result;
  }
  function replaceSelection(target, getters, text) {
    const value = apply(getters.value.get, target, []);
    const start = apply(getters.selectionStart, target, []);
    const end = apply(getters.selectionEnd, target, []);
    if (start === null || end === null) {
      apply(getters.value.set, target, [value + text]);
      return;
    }
    apply(getters.value.set, target, [
      apply(slice, value, [0, start]) + text + apply(slice, value, [end]),
    ]);
    const caret = start + text.length;
    apply(getters.setSelectionRange, target, [caret, caret, "none"]);
  }
  // The driver installs before public geometry methods; use the captured native op.
  function targetCenter(target) {
    const box = elementGeometry(target).bounds;
    cursorX = box[0] + box[2] / 2;
    cursorY = box[1] + box[3] / 2;
  }
  async function edit(target, text, run) {
    const getters = editingControl(target);
    if (!getters || apply(getters.disabled, target, []) || apply(getters.readOnly, target, []))
      return;
    for (const char of text) {
      if (run !== activeRun) return;
      const code = /^[a-z]$/i.test(char)
        ? "Key" + char.toUpperCase()
        : /^[0-9]$/.test(char)
          ? "Digit" + char
          : char === " "
            ? "Space"
            : "";
      const key = {
        key: char,
        code,
        keyCode: char.toUpperCase().charCodeAt(0),
        bubbles: true,
        cancelable: true,
        composed: true,
      };
      const down = fire(target, "KeyboardEvent", "keydown", key);
      if (
        down &&
        fire(target, "KeyboardEvent", "keypress", {
          ...key,
          charCode: char.charCodeAt(0),
          keyCode: char.charCodeAt(0),
        })
      ) {
        const init = { data: char, inputType: "insertText", bubbles: true, composed: true };
        if (fire(target, "InputEvent", "beforeinput", { ...init, cancelable: true })) {
          replaceSelection(target, getters, char);
          fire(target, "InputEvent", "input", init);
        }
      }
      await wait(110);
      if (run !== activeRun) return;
      fire(target, "KeyboardEvent", "keyup", key);
    }
  }
  function controlState(target) {
    for (let kind = 0; kind < controlGetters.length; kind++) {
      const getters = controlGetters[kind];
      try {
        return {
          form: apply(getters.form, target, []),
          type: apply(getters.type, target, []),
          kind,
        };
      } catch {}
    }
    return null;
  }
  function submit(target) {
    const state = controlState(target);
    const blocking =
      /^(text|search|tel|url|email|password|date|month|week|time|datetime-local|number)$/;
    if (!state || state.kind !== 0 || !blocking.test(state.type) || !state.form) return;
    let fields = 0;
    const candidates = controls("input,button");
    for (let index = 0; index < candidates.length; index++) {
      const candidate = candidates[index],
        value = controlState(candidate);
      if (value?.form !== state.form) continue;
      if (value.type === "submit" || (value.kind === 0 && value.type === "image")) {
        if (!apply(matches, candidate, [":disabled"]))
          fire(candidate, "PointerEvent", "click", {
            bubbles: true,
            cancelable: true,
            composed: true,
          });
        return;
      }
      if (value.kind === 0 && blocking.test(value.type)) fields++;
    }
    if (fields <= 1) apply(requestSubmit, state.form, []);
  }
  async function input(steps) {
    if (!Array.isArray(steps)) throw new TypeError("Input steps must be an array");
    const run = ++activeRun;
    for (const step of steps) {
      if (run !== activeRun) return;
      if (step.inputGuard != null) {
        inputGuard(step.inputGuard);
        continue;
      }
      if (step.wait != null) {
        await wait(Math.max(0, Number(step.wait)));
        continue;
      }
      if (step.focus) {
        focus.call(element(step.focus));
        continue;
      }
      if (step.move) {
        const target = element(step.move.selector);
        targetCenter(target);
        const init = {
          clientX: cursorX,
          clientY: cursorY,
          bubbles: true,
          composed: true,
          pointerType: "mouse",
          isPrimary: true,
          pointerId: 1,
        };
        if (hovered !== target) {
          if (hovered) {
            fire(hovered, "PointerEvent", "pointerout", { ...init, relatedTarget: target });
            fire(hovered, "MouseEvent", "mouseout", { ...init, relatedTarget: target });
          }
          fire(target, "PointerEvent", "pointerover", { ...init, relatedTarget: hovered });
          fire(target, "MouseEvent", "mouseover", { ...init, relatedTarget: hovered });
          fire(target, "PointerEvent", "pointerenter", {
            ...init,
            bubbles: false,
            relatedTarget: hovered,
          });
          fire(target, "MouseEvent", "mouseenter", {
            ...init,
            bubbles: false,
            relatedTarget: hovered,
          });
          hovered = target;
        }
        fire(target, "PointerEvent", "pointermove", init);
        fire(target, "MouseEvent", "mousemove", init);
        continue;
      }
      if (step.click) {
        const target = element(step.click);
        targetCenter(target);
        const init = {
          clientX: cursorX,
          clientY: cursorY,
          bubbles: true,
          cancelable: true,
          composed: true,
          detail: 1,
          pointerId: 1,
          pointerType: "mouse",
          isPrimary: true,
        };
        if (
          fire(target, "PointerEvent", "pointerdown", { ...init, buttons: 1, pressure: 0.5 }) &&
          fire(target, "MouseEvent", "mousedown", { ...init, buttons: 1 })
        )
          focus.call(target);
        fire(target, "PointerEvent", "pointerup", init);
        fire(target, "MouseEvent", "mouseup", init);
        fire(target, "PointerEvent", "click", init);
        continue;
      }
      const target = apply(activeElement, document, []);
      if (step.type != null) {
        await edit(target, String(step.type), run);
        continue;
      }
      if (step.press != null) {
        const key = String(step.press),
          keyCode = key === "Enter" ? 13 : key === "Tab" ? 9 : key === "Escape" ? 27 : 0;
        const init = { key, code: key, keyCode, bubbles: true, cancelable: true, composed: true };
        const down = fire(target, "KeyboardEvent", "keydown", init);
        if (
          down &&
          key === "Enter" &&
          fire(target, "KeyboardEvent", "keypress", { ...init, charCode: 13 })
        )
          submit(target);
        fire(target, "KeyboardEvent", "keyup", init);
        continue;
      }
      throw new TypeError("Unsupported input step");
    }
  }
  ops.op_set_host_input_driver(input, cancel);
})();
