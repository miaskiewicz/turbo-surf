(() => {
  const storage = localStorage;
  storage.clear();
  const rows = {};
  const attempt = (callback) => {
    try {
      return callback();
    } catch (error) {
      return { error: error.name };
    }
  };
  storage.setItem("first", "one");
  storage.second = "two";
  storage.setItem("first", "updated");
  rows.basic = [
    storage.length,
    storage.key(0),
    storage.key(1),
    storage.key(2),
    storage.first,
    storage.getItem("missing"),
  ];
  rows.keys = Object.keys(storage).sort();
  rows.descriptor = Object.getOwnPropertyDescriptor(storage, "first");
  rows.brand = Object.prototype.toString.call(storage);
  rows.stable = storage === localStorage;
  rows.invalid = [
    attempt(() => Storage.prototype.getItem.call({}, "first")),
    attempt(() => storage.setItem(Symbol(), "bad")),
    attempt(() => storage.setItem("bad", Symbol())),
    attempt(() => storage.getItem()),
    attempt(() => storage.setItem("bad")),
    attempt(() => new Storage()),
  ];
  storage.setItem("getItem", "shadow");
  rows.shadow = {
    method: typeof storage.getItem,
    own: Object.hasOwn(storage, "getItem"),
    descriptor: Object.getOwnPropertyDescriptor(storage, "getItem"),
    stored: Storage.prototype.getItem.call(storage, "getItem"),
    keys: Object.keys(storage).sort(),
  };
  rows.define = attempt(() => {
    Object.defineProperty(storage, "defined", { value: "value", configurable: true });
    return [storage.getItem("defined"), Object.getOwnPropertyDescriptor(storage, "defined")];
  });
  rows.defineAccessor = attempt(() =>
    Object.defineProperty(storage, "getter", {
      get() {
        return "value";
      },
    }),
  );
  delete storage.second;
  rows.deleted = [storage.length, storage.getItem("second")];
  sessionStorage.clear();
  sessionStorage.setItem("first", "session");
  rows.separate = [storage.first, sessionStorage.first];
  storage.clear();
  sessionStorage.clear();
  return JSON.stringify(rows);
})();
