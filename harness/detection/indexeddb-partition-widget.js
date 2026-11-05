(async () => {
  // The 127.0.0.1 frame of indexeddb-partition-probe.js: what its own IDBFactory and
  // its StorageAccessHandle's IDBFactory hold under `name`, then a write to each.
  const query = new URLSearchParams(location.search);
  const name = query.get("name"),
    step = query.get("step");
  const open = (factory) =>
    new Promise((resolve, reject) => {
      const request = factory.open(name, 1);
      request.onupgradeneeded = () => request.result.createObjectStore("s");
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  const exchange = async (factory, value) => {
    const listed = (await factory.databases()).some((entry) => entry.name === name);
    const db = await open(factory);
    const previous = await new Promise((resolve) => {
      const get = db.transaction("s").objectStore("s").get("who");
      get.onsuccess = () => resolve(get.result ?? null);
    });
    await new Promise((resolve) => {
      const transaction = db.transaction("s", "readwrite");
      transaction.objectStore("s").put(value, "who");
      transaction.oncomplete = resolve;
    });
    db.close();
    return { listed, previous };
  };
  const result = { step, own: await exchange(indexedDB, step) };
  try {
    const handle = await document.requestStorageAccess({ indexedDB: true });
    const factory = handle.indexedDB;
    result.handle = {
      tag: Object.prototype.toString.call(factory),
      same: factory === indexedDB,
      sameObject: factory === handle.indexedDB,
      ...(await exchange(factory, `${step}-handle`)),
    };
  } catch (error) {
    result.handle = { error: error.name };
  }
  parent.postMessage(result, "*");
})();
