(async () => {
  const result = { records: [] };
  for (const name of [
    "geolocation",
    "camera",
    "microphone",
    "midi",
    "clipboard-read",
    "clipboard-write",
    "notifications",
  ]) {
    const status = await navigator.permissions.query({ name });
    result.records.push({
      requested: name,
      name: status.name,
      state: status.state,
      brand: Object.prototype.toString.call(status),
      ownKeys: Reflect.ownKeys(status).map(String),
      target: status instanceof EventTarget,
      constructor: status.constructor.name,
      constructorIdentity: status.constructor === PermissionStatus,
    });
    if (name === "geolocation") {
      const prototype = Object.getPrototypeOf(status);
      result.prototype = Object.getOwnPropertyNames(prototype);
      result.parent = Object.getPrototypeOf(prototype) === EventTarget.prototype;
      result.constructorParent = Object.getPrototypeOf(PermissionStatus) === EventTarget;
      result.members = [];
      for (const key of ["name", "state", "onchange"]) {
        const member = Object.getOwnPropertyDescriptor(prototype, key);
        const errors = [];
        for (const receiver of [null, {}, new Proxy(status, {})]) {
          try {
            member.get.call(receiver);
            errors.push("accepted");
          } catch (error) {
            errors.push(error.name);
          }
        }
        result.members.push({
          key,
          enumerable: member.enumerable,
          configurable: member.configurable,
          getName: member.get.name,
          getLength: member.get.length,
          getPrototype: Object.hasOwn(member.get, "prototype"),
          setName: member.set?.name ?? null,
          setLength: member.set?.length ?? null,
          errors,
        });
      }
      let newError = null;
      try {
        new PermissionStatus();
      } catch (error) {
        newError = error.name;
      }
      result.newError = newError;
      result.initialHandler = status.onchange;
      const events = [];
      status.addEventListener("change", (event) =>
        events.push(["listener", event.target === status, event.isTrusted]),
      );
      status.onchange = function (event) {
        events.push(["handler", this === status, event.type]);
      };
      result.dispatch = status.dispatchEvent(new Event("change"));
      result.events = events;
      status.onchange = 123;
      result.numberHandler = status.onchange;
      status.onchange = { handleEvent() {} };
      result.objectHandler = typeof status.onchange;
      status.onchange = null;
      result.finalHandler = status.onchange;
    }
  }
  return result;
})();
