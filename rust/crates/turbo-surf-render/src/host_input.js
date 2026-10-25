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
  const pointer = (command) => ops.op_host_pointer(document, command);
  const parentOf = Object.getOwnPropertyDescriptor(Node.prototype, "parentNode").get;
  const nodeTypeOf = Object.getOwnPropertyDescriptor(Node.prototype, "nodeType").get;
  const shadowHost = Object.getOwnPropertyDescriptor(ShadowRoot.prototype, "host").get;
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
  // The mouse pointer: the elements under it for pointer and for compatibility mouse
  // boundaries, its position (null before the first event), pressed buttons, the
  // pointerdown target a click resolves against, the client position a pointer lock
  // froze, and whether a canceled pointerdown suppresses compatibility mouse events.
  let hovered = null,
    mouseHovered = null,
    cursorX = 0,
    cursorY = 0,
    position = null,
    pressed = 0,
    downTarget = null,
    frozen = null,
    movementBase = null,
    suppressed = false;
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
  // Shadow-including element ancestors, the target first. The driver's own arrays are
  // indexed directly: guest Array.prototype replacements never reach these steps.
  function ancestors(node) {
    const chain = [];
    while (node) {
      chain[chain.length] = node;
      const parent = apply(parentOf, node, []);
      if (!parent) break;
      const type = apply(nodeTypeOf, parent, []);
      node = type === 1 ? parent : type === 11 ? shadowParent(parent) : null;
    }
    return chain;
  }
  function shadowParent(fragment) {
    try {
      return apply(shadowHost, fragment, []);
    } catch {
      return null;
    }
  }
  const boundaryTypes = {
    pointer: ["PointerEvent", "pointerout", "pointerleave", "pointerover", "pointerenter"],
    mouse: ["MouseEvent", "mouseout", "mouseleave", "mouseover", "mouseenter"],
  };
  const scopedInit = { bubbles: false, cancelable: false, composed: false };
  // Chrome's boundary order: out at the old node, leave from it up to the common ancestor,
  // over at the new node, enter from below the common ancestor down to it. A node that
  // left the document receives nothing.
  function boundary(kind, from, to, init) {
    if (from && !apply(connected, from, [])) from = null;
    if (from === to) return to;
    const names = boundaryTypes[kind],
      type = names[0],
      out = names[1],
      leave = names[2],
      over = names[3],
      enter = names[4];
    init = { ...init, detail: 0 };
    const left = from ? ancestors(from) : [],
      entered = to ? ancestors(to) : [];
    let shared = 0;
    while (
      shared < left.length &&
      shared < entered.length &&
      left[left.length - 1 - shared] === entered[entered.length - 1 - shared]
    )
      shared++;
    if (from) {
      fire(from, type, out, { ...init, relatedTarget: to });
      for (let index = 0; index < left.length - shared; index++)
        fire(left[index], type, leave, { ...init, ...scopedInit, relatedTarget: to });
    }
    if (to) {
      fire(to, type, over, { ...init, relatedTarget: from });
      for (let index = entered.length - shared - 1; index >= 0; index--)
        fire(entered[index], type, enter, { ...init, ...scopedInit, relatedTarget: from });
    }
    return to;
  }
  const pointerBoundary = (to, init) => (hovered = boundary("pointer", hovered, to, init));
  const mouseBoundary = (to, init) => (mouseHovered = boundary("mouse", mouseHovered, to, init));
  // The click target: the capture or lock target a released pointerup went to, else the
  // nearest common inclusive ancestor of the pointerdown and pointerup targets.
  function clickTarget(down, up) {
    if (!down) return null;
    const chain = ancestors(down),
      candidates = ancestors(up);
    for (let index = 0; index < candidates.length; index++)
      for (let other = 0; other < chain.length; other++)
        if (chain[other] === candidates[index]) return candidates[index];
    return null;
  }
  const pointerTypes = { move: "pointermove", down: "pointerdown", up: "pointerup" },
    mouseTypes = { move: "mousemove", down: "mousedown", up: "mouseup" };
  // One trusted mouse event at the hit element: Chrome's PointerEventManager steps for the
  // pointer event (capture processing, lock/capture retargeting, boundaries) followed by the
  // compatibility mouse event and, for a release, implicit capture release and the click.
  function pointerAction(kind, hit) {
    const previous = position;
    targetCenter(hit);
    position = [cursorX, cursorY];
    const locked = pointer("locked");
    if (locked === null) {
      if (frozen !== null) movementBase = null;
      frozen = null;
    } else if (frozen === null) frozen = previous || position;
    const base = kind === "move" ? movementBase : null;
    movementBase = position;
    if (kind === "down") pressed = 1;
    if (kind === "up") pressed = 0;
    if (kind !== "move") pointer(kind === "down" ? "press" : "release");
    const client = frozen || position;
    const shared = {
      clientX: client[0],
      clientY: client[1],
      movementX: base ? position[0] - base[0] : 0,
      movementY: base ? position[1] - base[1] : 0,
      buttons: pressed,
      bubbles: true,
      cancelable: true,
      composed: true,
    };
    const init = {
      ...shared,
      button: kind === "move" ? -1 : 0,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      pressure: pressed ? 0.5 : 0,
    };
    const mouseInit = { ...shared, button: 0, detail: kind === "move" ? 0 : 1 };
    const captureInit = { ...init, cancelable: false };
    const processed = pointer("process"),
      lost = processed[0],
      got = processed[1];
    if (lost) fire(lost, "PointerEvent", "lostpointercapture", captureInit);
    if (got) {
      pointerBoundary(got, init);
      fire(got, "PointerEvent", "gotpointercapture", captureInit);
    }
    const captured = locked === null ? pointer("capture") : null;
    const target = locked || captured || hit;
    pointerBoundary(target, init);
    mouseBoundary(target, mouseInit);
    const proceed = fire(target, "PointerEvent", pointerTypes[kind], init);
    if (kind === "down") {
      suppressed = !proceed;
      downTarget = target;
    }
    const mouse = !suppressed && fire(target, "MouseEvent", mouseTypes[kind], mouseInit);
    if (kind === "down" && mouse) apply(focus, target, []);
    if (kind !== "up") return;
    suppressed = false;
    pointer("implicit");
    const released = pointer("process")[0];
    if (released) fire(released, "PointerEvent", "lostpointercapture", captureInit);
    const click = captured || locked || clickTarget(downTarget, target);
    downTarget = null;
    if (click)
      fire(click, "PointerEvent", "click", { ...init, isPrimary: false, button: 0, detail: 1 });
    if (released && locked === null) {
      pointerBoundary(hit, init);
      mouseBoundary(hit, mouseInit);
    }
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
        pointerAction("move", element(step.move.selector));
        continue;
      }
      if (step.down != null || step.up != null) {
        pointerAction(step.down != null ? "down" : "up", element(step.down ?? step.up));
        continue;
      }
      if (step.click) {
        const target = element(step.click);
        pointerAction("down", target);
        pointerAction("up", target);
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
