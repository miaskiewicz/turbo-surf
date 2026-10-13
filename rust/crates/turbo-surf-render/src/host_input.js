// Called through a V8 context slot owned by PageSession, never a guest global.
// Constructors, DOM methods and host dispatch are captured before page execution.
(() => {
  const document = globalThis.document;
  const query = document.querySelector.bind(document);
  const focus = HTMLElement.prototype.focus;
  const events = Object.fromEntries(
    ["Event", "SubmitEvent", "MouseEvent", "PointerEvent", "KeyboardEvent", "InputEvent"].map(
      (name) => [name, globalThis[name]],
    ),
  );
  const dispatch = ops.op_dispatch_host_event;
  const canceled = Object.getOwnPropertyDescriptor(Event.prototype, "defaultPrevented").get;
  const valueSetters = [HTMLInputElement, HTMLTextAreaElement].map((Constructor) => [
    Constructor,
    Object.getOwnPropertyDescriptor(Constructor.prototype, "value")?.set,
  ]);
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
    dispatch(event, target);
    return !canceled.call(event);
  }
  function element(selector) {
    const result = query(selector);
    if (!result) throw new Error("Missing input selector " + selector);
    return result;
  }
  async function edit(target, text, run) {
    const setter = valueSetters.find(([Constructor]) => target instanceof Constructor)?.[1];
    if (!setter || target.disabled || target.readOnly) return;
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
          setter.call(target, String(target.value || "") + char);
          fire(target, "InputEvent", "input", init);
        }
      }
      await wait(110);
      if (run !== activeRun) return;
      fire(target, "KeyboardEvent", "keyup", key);
    }
  }
  function submit(target) {
    const form = target.form;
    if (!form) return;
    if (!fire(form, "SubmitEvent", "submit", { bubbles: true, cancelable: true })) return;
    const method = (form.getAttribute("method") || "get").toLowerCase();
    if (method !== "get")
      throw new Error("Native input form navigation currently supports GET forms");
    const url = new URL(form.getAttribute("action") || location.href, document.baseURI);
    url.search = "";
    for (const control of form.querySelectorAll("input[name],textarea[name],select[name]")) {
      const type = (control.getAttribute("type") || "").toLowerCase();
      if (
        control.disabled ||
        ["submit", "button", "reset", "file"].includes(type) ||
        (["checkbox", "radio"].includes(type) && !control.checked)
      )
        continue;
      url.searchParams.append(control.getAttribute("name"), control.value ?? "");
    }
    location.assign(url.href);
  }
  async function input(steps) {
    if (!Array.isArray(steps)) throw new TypeError("Input steps must be an array");
    const run = ++activeRun;
    for (const step of steps) {
      if (run !== activeRun) return;
      if (step.wait != null) {
        await wait(Math.max(0, Number(step.wait)));
        continue;
      }
      if (step.focus) {
        focus.call(element(step.focus));
        continue;
      }
      if (step.move) {
        const target = element(step.move.selector),
          rectangle = target.getBoundingClientRect();
        cursorX = rectangle.left + rectangle.width / 2;
        cursorY = rectangle.top + rectangle.height / 2;
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
      const target = document.activeElement;
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
