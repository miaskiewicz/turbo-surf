// Read-only browser iterator metadata plus live collection mutation contracts.
(() => {
  const shape = (value) => {
    const prototypes = [];
    for (let object = value; object; object = Object.getPrototypeOf(object)) {
      prototypes.push({
        tag: Object.prototype.toString.call(object),
        keys: Reflect.ownKeys(object).map(String).sort(),
      });
      if (prototypes.length === 4) break;
    }
    const next = Object.getPrototypeOf(value).next;
    let invalid;
    try {
      next.call({});
      invalid = false;
    } catch (error) {
      invalid = error.name;
    }
    return {
      tag: Object.prototype.toString.call(value),
      own: Reflect.ownKeys(value).map(String),
      prototypes,
      next: {
        name: next.name,
        length: next.length,
        source: Function.prototype.toString.call(next),
        own: Reflect.ownKeys(next).map(String),
      },
      iterable: value[Symbol.iterator]() === value,
      invalid,
    };
  };
  const headers = new Headers([["a", "one"]]),
    params = new URLSearchParams("a=one");
  const result = { headers: shape(headers.entries()), params: shape(params.entries()) };
  for (const [name, collection] of [
    ["headers", headers],
    ["params", params],
  ]) {
    const iterator = collection.entries(),
      first = iterator.next();
    name === "headers" ? collection.append("b", "two") : collection.append("b", "two");
    result[name].live = [first, iterator.next(), iterator.next()];
  }
  return JSON.stringify(result);
})();
