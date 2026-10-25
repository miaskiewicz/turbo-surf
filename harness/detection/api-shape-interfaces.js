// Expanded own-member collector: the 75 interfaces of api-shape.js plus later parity
// targets. It measures only constructors, using byte-identical descriptor helpers so
// its prototypeMembers match api-shape.js; globals, methods and contracts stay there.
// Queries are ours; nothing here identifies which differences a site actually uses.
(() => {
  const safe = (fn) => {
    try {
      return fn();
    } catch (error) {
      return { error: error.name };
    }
  };
  const keys = (value) => Reflect.ownKeys(value).map(String).sort();
  const constructible = (fn) => {
    try {
      Reflect.construct(function () {}, [], fn);
      return true;
    } catch {
      return false;
    }
  };
  const shape = (fn) =>
    typeof fn === "function"
      ? {
          name: fn.name,
          length: fn.length,
          ownKeys: keys(fn),
          ownKeyOrder: Reflect.ownKeys(fn).map(String),
          constructible: constructible(fn),
          source: Function.prototype.toString.call(fn),
          functionPrototype: Object.getPrototypeOf(fn) === Function.prototype,
          nameDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "name")),
          lengthDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "length")),
          prototypeDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "prototype")),
        }
      : { type: typeof fn };
  const descriptor = (d) => {
    if (!d) return null;
    const result = { configurable: d.configurable, enumerable: d.enumerable };
    if ("value" in d) {
      result.kind = "data";
      result.writable = d.writable;
      result.valueType = typeof d.value;
    } else {
      result.kind = "accessor";
      result.get = shape(d.get);
      result.set = shape(d.set);
    }
    return result;
  };
  const members = (prototype) =>
    Object.fromEntries(
      Reflect.ownKeys(prototype).map((key) => {
        const own = Object.getOwnPropertyDescriptor(prototype, key);
        const entry = descriptor(own);
        if (typeof own.value === "function") entry.function = shape(own.value);
        return [String(key), entry];
      }),
    );
  const data = { constructors: {} };
  for (const name of [
    "Date",
    "Node",
    "Element",
    "DocumentFragment",
    "ShadowRoot",
    "HTMLElement",
    "HTMLCanvasElement",
    "HTMLFormElement",
    "HTMLInputElement",
    "HTMLButtonElement",
    "HTMLTextAreaElement",
    "HTMLSelectElement",
    "HTMLOutputElement",
    "HTMLMeterElement",
    "HTMLProgressElement",
    "HTMLLabelElement",
    "HTMLCollection",
    "NodeList",
    "HTMLFormControlsCollection",
    "RadioNodeList",
    "Blob",
    "File",
    "FormData",
    "FormDataEvent",
    "CustomStateSet",
    "ValidityState",
    "UserActivation",
    "Document",
    "Range",
    "AbstractRange",
    "NodeRange",
    "StaticRange",
    "CustomElementRegistry",
    "ElementInternals",
    "MutationObserver",
    "Navigator",
    "Performance",
    "Storage",
    "URL",
    "Request",
    "Response",
    "Event",
    "EventTarget",
    "MessageEvent",
    "MessageChannel",
    "MessagePort",
    "SubmitEvent",
    "AbortController",
    "AbortSignal",
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "WritableStream",
    "WritableStreamDefaultWriter",
    "TransformStream",
    "TrustedHTML",
    "TrustedScript",
    "TrustedScriptURL",
    "TrustedTypePolicy",
    "TrustedTypePolicyFactory",
    "ErrorEvent",
    "DOMException",
    "CSSStyleSheet",
    "StyleSheet",
    "MediaList",
    "Headers",
    "URLSearchParams",
    "TextEncoder",
    "TextDecoder",
    "Crypto",
    "SubtleCrypto",
    "DOMMatrix",
    "DOMMatrixReadOnly",
    "DOMPoint",
    "DOMPointReadOnly",
    "HTMLAllCollection",
    "HTMLOptionsCollection",
    "HTMLOptionElement",
    "HTMLOptGroupElement",
    "HTMLDataListElement",
    "HTMLDialogElement",
    "HTMLDetailsElement",
    "HTMLTemplateElement",
    "HTMLSlotElement",
    "HTMLFieldSetElement",
    "HTMLLegendElement",
    "HTMLIFrameElement",
    "HTMLImageElement",
    "HTMLAnchorElement",
    "HTMLScriptElement",
    "HTMLStyleElement",
    "HTMLLinkElement",
    "HTMLBodyElement",
    "HTMLHtmlElement",
    "HTMLDivElement",
    "HTMLMediaElement",
    "HTMLVideoElement",
    "HTMLAudioElement",
    "HTMLObjectElement",
    "CharacterData",
    "Text",
    "Comment",
    "Attr",
    "NamedNodeMap",
    "DOMTokenList",
    "DOMRect",
    "DOMRectReadOnly",
    "DOMRectList",
    "Selection",
    "Sanitizer",
    "CommandEvent",
    "ToggleEvent",
    "CustomEvent",
    "UIEvent",
    "FocusEvent",
    "MouseEvent",
    "PointerEvent",
    "KeyboardEvent",
    "InputEvent",
    "WheelEvent",
    "PopStateEvent",
    "HashChangeEvent",
    "CSSStyleDeclaration",
    "CSSRule",
    "CSSStyleRule",
    "CSSRuleList",
    "StyleSheetList",
    "StylePropertyMap",
    "StylePropertyMapReadOnly",
    "Animation",
    "AnimationEffect",
    "KeyframeEffect",
    "AnimationTimeline",
    "DocumentTimeline",
    "ViewTransition",
    "EditContext",
    "CaretPosition",
    "VisualViewport",
    "Window",
    "Location",
    "History",
    "Screen",
    "ScreenOrientation",
    "FileList",
    "FileReader",
    "DataTransfer",
    "ResizeObserver",
    "IntersectionObserver",
    "PerformanceObserver",
    "BroadcastChannel",
    "XMLHttpRequest",
    "WebSocket",
    "Permissions",
    "PermissionStatus",
  ]) {
    data.constructors[name] = safe(() => {
      const ctor = globalThis[name];
      return {
        shape: shape(ctor),
        globalDescriptor: descriptor(Object.getOwnPropertyDescriptor(globalThis, name)),
        prototypeDescriptor: descriptor(Object.getOwnPropertyDescriptor(ctor, "prototype")),
        prototypeTag: Object.prototype.toString.call(ctor.prototype),
        prototypeKeys: keys(ctor.prototype),
        prototypeOwnKeyOrder: Reflect.ownKeys(ctor.prototype).map(String),
        prototypeMembers: members(ctor.prototype),
      };
    });
  }
  return JSON.stringify(data);
})();
