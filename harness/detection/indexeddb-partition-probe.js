(async () => {
  // IndexedDB storage partitioning as Chrome 156 ships it (#148): the top-level page
  // writes its first-party database, then a 127.0.0.1 frame
  // (indexeddb-partition-widget.js) reads and writes the same name directly and
  // through a localhost frame (A -> B -> A), each in its own partition and through
  // its StorageAccessHandle's unpartitioned IDBFactory. Database names carry the
  // top-level host so a run never reads another run's databases.
  const run = location.hostname;
  const name = `partition-${run}`;
  const query = `name=${encodeURIComponent(name)}`;
  const open = () =>
    new Promise((resolve, reject) => {
      const request = indexedDB.open(name, 1);
      request.onupgradeneeded = () => request.result.createObjectStore("s");
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  const read = (db) =>
    new Promise((resolve) => {
      const get = db.transaction("s").objectStore("s").get("who");
      get.onsuccess = () => resolve(get.result ?? null);
    });
  const frame = (src) =>
    new Promise((resolve) => {
      const element = document.createElement("iframe");
      const listener = (event) => {
        if (event.source !== element.contentWindow) return;
        removeEventListener("message", listener);
        element.remove();
        resolve(event.data);
      };
      addEventListener("message", listener);
      element.src = src;
      document.body.append(element);
    });
  const db = await open();
  const previous = await read(db);
  await new Promise((resolve) => {
    const transaction = db.transaction("s", "readwrite");
    transaction.objectStore("s").put("top", "who");
    transaction.oncomplete = resolve;
  });
  db.close();
  const direct = await frame(`http://127.0.0.1:${location.port}/idb-widget?${query}&step=direct`);
  const nested = await frame(`http://localhost:${location.port}/idb-middle?${query}&step=nested`);
  const after = await open();
  const final = await read(after);
  after.close();
  return { run, top: { previous, final }, direct, nested };
})();
