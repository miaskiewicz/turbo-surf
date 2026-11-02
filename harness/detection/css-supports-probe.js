(() => {
  const cases = [
    ["display", "grid"],
    ["width", "1px"],
    ["width", "nonsense"],
    ["unknown", "1"],
    ["-moz-appearance", "none"],
    ["-webkit-transform", "translateX(1px)"],
    ["-webkit-user-select", "none"],
    ["WIDTH", "1px"],
    [" width ", "1px"],
    ["--token", "3"],
    ["width", undefined],
    ["(display: grid)"],
    ["display:grid"],
    ["(width: var(--x))"],
    ["(unknown: 1) or (display: flex)"],
    ["selector(div > .x)"],
    ["selector(:has(> span))"],
    ["selector(:is(div,span))"],
    ["selector(:bogus)"],
    ["selector(div,span)"],
    ["selector(div >)"],
    ["selector(*|p)"],
    ["selector(*|*)"],
    ["selector(|p)"],
    ["selector(ns|p)"],
    ["selector([*|title])"],
    ["selector(.\\31 23)"],
    ["selector(#\\66oo)"],
    ["selector(.a\\.b)"],
    ["selector(\\64iv > .x)"],
    ["selector(.é)"],
    ["selector(:hover)"],
    ["selector(::before)"],
    ["selector(div::after)"],
    ["selector(:is(:bogus))"],
    ["selector(:where(:bogus, p))"],
    ["selector(:not(:bogus))"],
    ["selector(:host)"],
    ["selector(::slotted(p))"],
    ["selector(:nth-child(2n of .x))"],
    ["selector(:empty)"],
    ["selector(:link)"],
    ["selector(:read-write)"],
    [],
    [Symbol("x")],
    ["width", Symbol("x")],
    [null],
    [undefined],
    [42],
    [["(display: grid)"]],
  ];
  const results = cases.map((args) => {
    try {
      return CSS.supports(...args);
    } catch (error) {
      return error.name;
    }
  });
  const trace = [];
  const first = {
    toString() {
      trace.push("first");
      return "width";
    },
  };
  const second = {
    toString() {
      trace.push("second");
      return "1px";
    },
  };
  const converted = CSS.supports(first, second);
  const descriptor = Object.getOwnPropertyDescriptor(CSS, "supports");
  let construct;
  try {
    new CSS.supports("(display:grid)");
  } catch (error) {
    construct = error.name;
  }
  const detached = CSS.supports;
  const unbound = detached.call(null, "width", "1px");
  const nativeString = String;
  globalThis.String = () => {
    throw new Error("public String must stay unreachable");
  };
  let poisoned;
  try {
    poisoned = CSS.supports("width", "1px");
  } finally {
    globalThis.String = nativeString;
  }
  const main = document.createElement("main");
  main.innerHTML = "<div class='x'></div><div class='y'></div>";
  document.body.append(main);
  const style = document.createElement("style");
  document.head.append(style);
  const selectors = ["main:has(> .x)", "main > :is(main > .x)", "main > :where(.y)"];
  const geometry = selectors.map((selector) => {
    style.textContent = `main,main > div{width:11px;height:7px}${selector}{width:43px}`;
    return [
      main.getBoundingClientRect().width,
      ...Array.from(main.children, (node) => node.getBoundingClientRect().width),
    ];
  });
  style.remove();
  main.remove();
  return {
    geometry,
    results,
    trace,
    converted,
    construct,
    unbound,
    poisoned,
    shape: {
      name: CSS.supports.name,
      length: CSS.supports.length,
      writable: descriptor.writable,
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
    },
  };
})();
