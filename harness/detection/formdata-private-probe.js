(() => {
  const fd = new FormData();
  fd.append("dup", "first");
  fd.append("other", "middle");
  fd.append("dup", "last");
  fd.set("dup", "replacement");
  const setOrder = [...fd];
  fd.append("usv\ud800", "v\udfff");
  const iterator = fd.entries();
  const first = iterator.next();
  fd.append("late", "value");
  const remaining = [...iterator];
  const done = iterator.next();
  fd.append("after-done", "x");
  const finalDone = iterator.next();
  const files = new FormData();
  const file = new File([new Uint8Array([1, 2])], "original", { lastModified: 42 });
  files.append("file", file);
  files.append("renamed", file, "new");
  files.append("blob", new Blob(["x"], { type: "TEXT/PLAIN" }));
  const date = {
    millis: file.lastModifiedDate.getTime(),
    fresh: file.lastModifiedDate !== file.lastModifiedDate,
  };
  const values = [...files].map(([key, value]) => ({
    key,
    name: value.name,
    size: value.size,
    type: value.type,
    same: value === file,
    modified: key === "blob" ? "clock" : value.lastModified,
  }));
  const errors = [];
  for (const fn of [
    () => FormData.prototype.get.call({}, "x"),
    () => fd.append("x"),
    () => fd.append("x", "text", "filename"),
    () => fd.get(Symbol()),
    () => fd.append("x", Symbol()),
  ]) {
    try {
      fn();
      errors.push("none");
    } catch (error) {
      errors.push(error.name);
    }
  }
  const foreach = [];
  const changing = new FormData();
  changing.append("a", "1");
  changing.forEach((value, key, owner) => {
    foreach.push([key, value, owner === changing]);
    if (key === "a") changing.append("b", "2");
  });
  const saved = { slice: Array.prototype.slice, create: Object.create, TypeError };
  let poisoned;
  try {
    Array.prototype.slice = () => {
      throw new Error("guest slice");
    };
    Object.create = () => {
      throw new Error("guest create");
    };
    globalThis.TypeError = class Replacement extends Error {};
    const value = FormData.prototype.entries.call(fd).next().value;
    let brand;
    try {
      Object.getPrototypeOf(FormData.prototype.entries.call(fd)).next.call({});
    } catch (error) {
      brand = error.name;
    }
    poisoned = { value, brand };
  } finally {
    Array.prototype.slice = saved.slice;
    Object.create = saved.create;
    globalThis.TypeError = saved.TypeError;
  }
  const ownKeys = (prototype) =>
    Reflect.ownKeys(prototype).map((key) => (typeof key === "symbol" ? key.toString() : key));
  const members = { formdata: ownKeys(FormData.prototype), file: ownKeys(File.prototype) };
  const frame = document.createElement("iframe");
  document.body.append(frame);
  const child = frame.contentWindow,
    foreignFile = new child.File(["x"], "foreign", { lastModified: 17 }),
    foreignData = new child.FormData();
  foreignData.append("foreign", foreignFile);
  const parentName = Object.getOwnPropertyDescriptor(File.prototype, "name").get;
  const cross = {
    distinct: child.FormData !== FormData && child.File !== File,
    name: parentName.call(foreignFile),
    same: FormData.prototype.get.call(foreignData, "foreign") === foreignFile,
  };
  FormData.prototype.append.call(foreignData, "parent", "ok");
  frame.remove();
  cross.retained = FormData.prototype.get.call(foreignData, "parent");
  cross.retainedName = parentName.call(foreignFile);
  return {
    setOrder,
    members,
    date,
    cross,
    poisoned,
    first,
    remaining,
    done,
    finalDone,
    values,
    errors,
    foreach,
    own: Object.keys(fd),
    tag: Object.prototype.toString.call(fd),
    iteratorTag: Object.prototype.toString.call(fd.entries()),
    descriptor: Object.getOwnPropertyDescriptor(FormData.prototype, Symbol.iterator).enumerable,
    alias: FormData.prototype.entries === FormData.prototype[Symbol.iterator],
  };
})();
