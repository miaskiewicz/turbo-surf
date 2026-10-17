// Authored post-verdict DOMTokenList/ARIA reflection collector, shared with the
// normal Chrome localhost oracle. It removes every fixture element before returning.
(() => {
  const safe = (fn) => {
    try {
      return fn();
    } catch (e) {
      return { error: e.name };
    }
  };
  const el = document.createElement("div");
  document.body.appendChild(el);
  el.className = " a a\tb  c ";
  const list = el.classList;
  const token = {
    identity: list === el.classList,
    tag: Object.prototype.toString.call(list),
    keys: Reflect.ownKeys(list).map(String),
    value: list.value,
    items: Array.from(list),
  };
  token.errors = {};
  for (const [name, op] of Object.entries({
    empty: () => list.add(""),
    spaces: () => list.add("a b"),
    containsEmpty: () => list.contains(""),
    containsSpace: () => list.contains("a b"),
    supports: () => list.supports("a"),
    supportEmpty: () => list.supports(""),
    partSupports: () => el.part.supports("a"),
    missingContains: () => list.contains(),
    missingToggle: () => list.toggle(),
    replaceMissing: () => list.replace("a"),
  }))
    token.errors[name] = safe(op);
  list.toggle("b", undefined);
  token.forceUndefined = { value: list.value, items: Array.from(list) };
  list.replace("a", "c");
  token.replaceDuplicate = { value: list.value, items: Array.from(list) };
  list.value = " x x\ny ";
  token.valueSet = { value: list.value, items: Array.from(list) };
  el.part = "one two";
  token.part = {
    identity: el.part === el.part,
    items: Array.from(el.part),
    attribute: el.getAttribute("part"),
  };
  const a = document.createElement("span"),
    b = document.createElement("span");
  a.id = "aria-a";
  b.id = "aria-b";
  document.body.append(a, b);
  const aria = {};
  for (const property of ["ariaControlsElements", "ariaActiveDescendantElement"]) {
    const many = property.endsWith("Elements"),
      attr = many ? "aria-controls" : "aria-activedescendant";
    const report = {};
    report.initial = el[property];
    el.setAttribute(attr, many ? "aria-a missing aria-b aria-a" : "aria-a");
    let first = el[property];
    report.attribute = many ? first.map((e) => e.id) : first?.id;
    report.same = el[property] === first;
    report.frozen = many ? Object.isFrozen(first) : null;
    el[property] = many ? [b, a, b] : b;
    first = el[property];
    report.explicit = {
      attribute: el.getAttribute(attr),
      value: many ? first.map((e) => e.id) : first?.id,
      same: el[property] === first,
      frozen: many ? Object.isFrozen(first) : null,
    };
    el.setAttribute(attr, "");
    report.reset = many ? el[property]?.map((e) => e.id) : (el[property]?.id ?? null);
    el[property] = null;
    report.null = { attribute: el.getAttribute(attr), value: el[property] };
    report.invalid = safe(() => {
      el[property] = many ? [a, {}] : {};
      return "accepted";
    });
    report.afterInvalid = el.getAttribute(attr);
    aria[property] = report;
  }
  const detached = document.createElement("b");
  el.ariaControlsElements = [detached];
  aria.detached = {
    assigned: el.ariaControlsElements.map((e) => e === detached),
    attribute: el.getAttribute("aria-controls"),
  };
  const host = document.createElement("div");
  document.body.append(host);
  const shadow = host.attachShadow({ mode: "open" }),
    child = document.createElement("div");
  shadow.append(child);
  child.ariaControlsElements = [a];
  aria.childToParent = child.ariaControlsElements.map((e) => e.id);
  el.ariaControlsElements = [child];
  aria.parentToChild = el.ariaControlsElements.map((e) => e.id);
  child.setAttribute("aria-controls", "aria-a");
  aria.shadowAttribute = child.ariaControlsElements.map((e) => e.id);
  el.remove();
  a.remove();
  b.remove();
  host.remove();
  return JSON.stringify({ token, aria });
})();
