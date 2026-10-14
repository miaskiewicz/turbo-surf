(() => {
  const rows = [];
  const marker = (name, value) => rows.push({ name, value });
  globalThis.__dynamicMarks = [];
  const append = (name, source, target = document.body) => {
    const script = document.createElement("script");
    script.id = name;
    script.textContent = source;
    target.appendChild(script);
    marker(name, globalThis.__dynamicMarks.slice());
    return script;
  };
  const first = append("immediate", "__dynamicMarks.push(document.currentScript.id)");
  first.remove();
  document.body.appendChild(first);
  marker("reinsert", globalThis.__dynamicMarks.slice());
  const empty = document.createElement("script");
  document.body.appendChild(empty);
  empty.textContent = "__dynamicMarks.push('late-text')";
  marker("late-text", globalThis.__dynamicMarks.slice());
  const chars = document.createElement("script");
  const text = document.createTextNode("");
  chars.appendChild(text);
  document.body.appendChild(chars);
  text.data = "__dynamicMarks.push('character-data')";
  marker("character-data", globalThis.__dynamicMarks.slice());
  const data = document.createElement("script");
  data.type = "application/json";
  data.textContent = "__dynamicMarks.push('data-block')";
  document.body.appendChild(data);
  data.type = "text/javascript";
  marker("type-change", globalThis.__dynamicMarks.slice());
  // An equal textContent write triggers script preparation; changing data alone does not.
  // oxlint-disable-next-line no-self-assign
  data.textContent = data.textContent;
  marker("data-write", globalThis.__dynamicMarks.slice());
  const inert = document.createElement("div");
  inert.innerHTML = "<script>__dynamicMarks.push('innerhtml')</script>";
  document.body.appendChild(inert);
  marker("innerhtml", globalThis.__dynamicMarks.slice());
  const clone = first.cloneNode(true);
  document.body.appendChild(clone);
  marker("clone-started", globalThis.__dynamicMarks.slice());
  const foreign = document.implementation.createHTMLDocument("inert");
  const foreignScript = foreign.createElement("script");
  foreignScript.textContent = "__dynamicMarks.push('adopted')";
  foreign.body.appendChild(foreignScript);
  document.body.appendChild(foreignScript);
  marker("adopt-inert-started", globalThis.__dynamicMarks.slice());
  const guarded = document.createElement("script");
  guarded.textContent = "__dynamicMarks.push('private-preparation')";
  guarded.__tcDone = true;
  let reads = 0;
  for (const name of ["ownerDocument", "textContent", "getAttribute"])
    Object.defineProperty(guarded, name, {
      configurable: true,
      get() {
        reads++;
        throw Error("public script hook");
      },
    });
  const savedJSONParse = JSON.parse,
    savedIterator = Array.prototype[Symbol.iterator];
  JSON.parse = () => {
    reads++;
    throw Error("public JSON hook");
  };
  Array.prototype[Symbol.iterator] = () => {
    reads++;
    throw Error("public iterator hook");
  };
  try {
    document.body.appendChild(guarded);
  } finally {
    JSON.parse = savedJSONParse;
    Array.prototype[Symbol.iterator] = savedIterator;
  }
  marker("private-preparation", { reads, marks: globalThis.__dynamicMarks.slice() });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const shadow = host.attachShadow({ mode: "open" });
  append(
    "shadow-inline",
    "__dynamicMarks.push(document.currentScript === null ? 'shadow-null' : 'shadow-script')",
    shadow,
  );
  const errors = [];
  function onError(event) {
    errors.push({ message: event.error.message, current: document.currentScript?.id || null });
    event.preventDefault();
  }
  addEventListener("error", onError);
  append("throwing", "throw new Error('authored dynamic error')");
  removeEventListener("error", onError);
  marker("error-event", errors);
  marker("outside-current-script", document.currentScript === null);
  return rows;
})();
