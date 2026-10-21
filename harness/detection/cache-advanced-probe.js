(async () => {
  const result = {};
  const read = async (url, range, cache = "default") => {
    try {
      const response = await fetch(url, {
        mode: "same-origin",
        cache,
        headers: range ? { Range: range } : {},
      });
      return {
        status: response.status,
        body: await response.text(),
        count: response.headers.get("x-count"),
        range: response.headers.get("content-range"),
      };
    } catch (error) {
      return { error: error.name };
    }
  };
  result.full = [
    await read("/range/full"),
    await read("/range/full", "bytes=2-5", "only-if-cached"),
    await read("/range/full", "bytes=-4", "only-if-cached"),
    await read("/range/full", "bytes=5-", "only-if-cached"),
    await read("/range/full", "bytes=20-30", "only-if-cached"),
    await read("/range/full", null, "only-if-cached"),
  ];
  result.fullRange = [
    await read("/range/full-range"),
    await read("/range/full-range", "bytes=2-5"),
    await read("/range/full-range", "bytes=2-5", "force-cache"),
    await read("/range/full-range", "bytes=2-5", "only-if-cached"),
  ];
  result.partial = [
    await read("/range/partial", "bytes=0-3"),
    await read("/range/partial", "bytes=0-3", "force-cache"),
    await read("/range/partial", "bytes=0-5", "only-if-cached"),
    await read("/range/partial", "bytes=4-9"),
    await read("/range/partial", null, "only-if-cached"),
  ];
  result.partialFull = [
    await read("/range/partial-full", "bytes=0-3"),
    await read("/range/partial-full", "bytes=4-9"),
    await read("/range/partial-full"),
    await read("/range/partial-full", null, "only-if-cached"),
  ];
  result.overlap = [
    await read("/range/overlap", "bytes=0-3"),
    await read("/range/overlap", "bytes=2-6"),
    await read("/range/overlap", "bytes=0-6", "only-if-cached"),
  ];
  for (const credentials of ["omit", "same-origin", "include"]) {
    const url = "/cache/purge-" + credentials;
    const first = await fetch(url);
    await first.text();
    const clear = await fetch("/clear?types=%22cache%22", { credentials });
    await clear.text();
    result["purge-" + credentials] = await read(url, null, "only-if-cached");
  }
  for (const types of ["cache", '"unknown"', '"*"']) {
    const url = "/cache/purge-types-" + encodeURIComponent(types);
    const first = await fetch(url);
    await first.text();
    const clear = await fetch("/clear?types=" + encodeURIComponent(types));
    await clear.text();
    result["types-" + types] = await read(url, null, "only-if-cached");
  }
  const initial = await fetch("/cache/nested");
  await initial.text();
  const frame = (url, key) =>
    new Promise((resolve) => {
      const child = document.createElement("iframe");

      const listener = (event) => {
        if (event.data?.kind === "cache-ancestor" && event.data.key === key) {
          window.removeEventListener("message", listener);
          child.remove();
          resolve(event.data.count);
        }
      };
      window.addEventListener("message", listener);
      child.src = url;
      document.body.append(child);
    });
  result.ancestor = [
    await frame("/frame-direct?key=direct", "direct"),
    await frame("http://localhost:" + location.port + "/frame-parent", "nested"),
  ];
  const load = (tag, attribute, url) =>
    new Promise((resolve, reject) => {
      const element = document.createElement(tag);
      element.onload = () => resolve(element);
      element.onerror = () => reject(new Error("resource failed: " + url));
      if (tag === "link") element.rel = "stylesheet";
      element[attribute] = url;
      document.head.append(element);
    });
  globalThis.__cacheStage = "classic";
  await load("script", "src", "/resource/classic");
  await load("script", "src", "/resource/classic");
  result.classic = resourceClassic;
  globalThis.__cacheStage = "style";
  await load("link", "href", "/resource/style");
  await load("link", "href", "/resource/style");
  result.style = getComputedStyle(document.body).marginLeft;
  globalThis.__cacheStage = "image";
  await load("img", "src", "/resource/image");
  await load("img", "src", "/resource/image");
  result.image = await (await fetch("/stats?path=%2Fresource%2Fimage")).json();
  const moduleFrame = (key) =>
    new Promise((resolve) => {
      const child = document.createElement("iframe");
      const listener = (event) => {
        if (event.data?.kind === "resource-module" && event.data.key === key) {
          window.removeEventListener("message", listener);
          child.remove();
          resolve(event.data.value);
        }
      };
      window.addEventListener("message", listener);
      child.src = "/module-frame?key=" + key;
      document.body.append(child);
    });
  globalThis.__cacheStage = "modules";
  result.modules = [await moduleFrame("first"), await moduleFrame("second")];
  return result;
})();
