(async () => {
  const previous = localStorage.getItem("partition-control");
  localStorage.setItem("partition-control", "widget");
  const result = {
    previous,
    value: localStorage.getItem("partition-control"),
    has: await document.hasStorageAccess(),
  };
  try {
    await document.requestStorageAccess();
    result.request = "resolved";
  } catch (e) {
    result.request = e.name;
  }
  try {
    const handle = await document.requestStorageAccess({ localStorage: true });
    result.handle = {
      tag: Object.prototype.toString.call(handle),
      hasLocal: typeof handle?.localStorage,
      localSame: handle?.localStorage === localStorage,
      unpartitionedValue: handle?.localStorage.getItem("partition-control"),
      sameObject: handle?.localStorage === handle?.localStorage,
    };
  } catch (e) {
    result.handle = { error: e.name };
  }
  parent.postMessage(result, "*");
})();
