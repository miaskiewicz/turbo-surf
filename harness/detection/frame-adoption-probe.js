(() => {
  const apply = Reflect.apply,
    define = Object.defineProperty,
    key = Symbol.for("ts");
  const privateNames = [
    "frameRecordFor",
    "frameRealm",
    "loadFrame",
    "prepareFrameAdoption",
    "syncFrameConnections",
    "accessFrame",
    "frameSourceChanged",
    "recaptchaBframeHandshake",
  ];
  const outer = document.createElement("iframe");
  document.body.appendChild(outer);
  const foreign = outer.contentWindow,
    inner = document.createElement("iframe");
  document.body.appendChild(inner);
  const oldInner = inner.contentWindow;
  const getter = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "contentWindow").get;
  const foreignNamespace = foreign[key] || {},
    foreignSaved = privateNames.map((name) =>
      Object.getOwnPropertyDescriptor(foreignNamespace, name),
    );
  let foreignHooks = 0;
  for (const name of privateNames)
    define(foreignNamespace, name, {
      __proto__: null,
      configurable: true,
      value() {
        foreignHooks++;
      },
    });
  let adoption;
  try {
    foreign.document.body.appendChild(inner);
    const newInner = apply(getter, inner, []);
    adoption = {
      hooks: foreignHooks,
      owner: inner.ownerDocument === foreign.document,
      distinct: newInner !== oldInner,
      borrowed: newInner === inner.contentWindow,
      retired: oldInner.closed,
      frameElement: newInner.frameElement === inner,
    };
  } finally {
    for (let index = 0; index < privateNames.length; index++) {
      if (foreignSaved[index]) define(foreignNamespace, privateNames[index], foreignSaved[index]);
      else delete foreignNamespace[privateNames[index]];
    }
    outer.remove();
  }
  return JSON.stringify(adoption);
})();
