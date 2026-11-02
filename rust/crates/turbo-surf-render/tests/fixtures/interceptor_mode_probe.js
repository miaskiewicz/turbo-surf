// Strict vs sloppy writes, deletes and definitions on legacy platform objects whose
// indexed/named properties are native interceptors: HTMLAllCollection (getters
// only), HTMLSelectElement and HTMLOptionsCollection (indexed setters). Chrome's
// interceptors throw their own TypeError only when V8 says the operation must
// throw (strict [[Set]]/[[Delete]], Object.defineProperty); sloppy writes and the
// Reflect forms fail quietly. Each operation is compiled with `Function`, so its
// language mode is exactly its own directive whatever the evaluator wraps around
// this probe.
(() => {
  document.body.innerHTML = '<p id="p">x</p><div id="host"></div>';
  const host = document.getElementById("host"),
    all = document.all,
    root = document.documentElement,
    result = {};
  const compile = (strict, body) =>
    Function("target", "option", (strict ? '"use strict";' : "") + body);
  // A fresh detached <option>c</option> (createElement: the operations, not the
  // Option() constructor, are under test).
  const option = () => {
    const element = document.createElement("option");
    element.text = "c";
    return element;
  };
  const outcome = (operation, target) => {
    try {
      const value = operation(target, option);
      return { threw: null, value: typeof value === "boolean" ? value : null };
    } catch (error) {
      return { threw: error instanceof TypeError ? "TypeError" : String(error && error.name) };
    }
  };
  // [name, strict, body]; `target` is the collection under test.
  const allCases = [
    ["SetSloppy", false, "target[0] = 1;"],
    ["SetStrict", true, "target[0] = 1;"],
    ["SetMissingSloppy", false, "target[999] = 1;"],
    ["SetMissingStrict", true, "target[999] = 1;"],
    ["SetNamedSloppy", false, "target.p = 1;"],
    ["SetNamedStrict", true, "target.p = 1;"],
    ["ReflectSet", false, "return Reflect.set(target, 0, 1);"],
    ["DeleteSloppy", false, "return delete target[0];"],
    ["DeleteStrict", true, "return delete target[0];"],
    ["DeleteNamedStrict", true, "return delete target.p;"],
    ["Define", false, "Object.defineProperty(target, 0, { value: 1, configurable: true });"],
    [
      "DefineMissing",
      false,
      "Object.defineProperty(target, 999, { value: 1, configurable: true });",
    ],
    [
      "ReflectDefine",
      false,
      "return Reflect.defineProperty(target, 0, { value: 1, configurable: true });",
    ],
  ];
  for (const [name, strict, body] of allCases) {
    result["all" + name] = {
      ...outcome(compile(strict, body), all),
      state: {
        first: all[0] === root,
        missing: 999 in all,
        named: all.p === document.getElementById("p"),
      },
    };
  }
  const selectCases = [
    ["SetSloppy", false, "target[2] = option();"],
    ["SetStrict", true, "target[2] = option();"],
    ["SetNullSloppy", false, "target[0] = null;"],
    ["SetNullStrict", true, "target[0] = null;"],
    ["SetWrongTypeSloppy", false, "target[0] = 1;"],
    ["SetWrongTypeStrict", true, "target[0] = 1;"],
    ["ReflectSet", false, "return Reflect.set(target, 2, option());"],
    [
      "DefineData",
      false,
      "Object.defineProperty(target, 2, { value: option(), configurable: true });",
    ],
    [
      "DefineAccessor",
      false,
      "Object.defineProperty(target, 2, { get() { return null; }, configurable: true });",
    ],
    [
      "ReflectDefineAccessor",
      false,
      "return Reflect.defineProperty(target, 2, { get() { return null; }, configurable: true });",
    ],
    ["DeleteSloppy", false, "return delete target[0];"],
    ["DeleteStrict", true, "return delete target[0];"],
    ["DeleteMissingStrict", true, "return delete target[9];"],
  ];
  for (const [label, pick] of [
    ["select", (select) => select],
    ["options", (select) => select.options],
  ]) {
    for (const [name, strict, body] of selectCases) {
      host.innerHTML = "<select><option>a</option><option>b</option></select>";
      const select = host.firstChild;
      result[label + name] = {
        ...outcome(compile(strict, body), pick(select)),
        state: {
          texts: Array.from(select.options, (option) => option.text).join(","),
          length: select.length,
        },
      };
    }
  }
  host.textContent = "";
  return result;
})();
