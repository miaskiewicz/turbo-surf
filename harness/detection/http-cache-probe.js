(async () => {
  const result = {};
  const read = async (url, cache = "default", extra = {}) => {
    try {
      const response = await fetch(url, { cache, mode: "same-origin", ...extra });
      return {
        status: response.status,
        body: await response.json(),
        count: response.headers.get("x-count"),
        merged: response.headers.get("x-merged"),
        redirected: response.redirected,
      };
    } catch (error) {
      return { error: error.name };
    }
  };
  for (const mode of [
    "default",
    "no-store",
    "reload",
    "no-cache",
    "force-cache",
    "only-if-cached",
  ]) {
    const url = "/cache/" + mode;
    result[mode] = [await read(url), await read(url, mode), await read(url)];
  }
  result.missing = await read("/cache/missing", "only-if-cached");
  for (const mode of ["default", "force-cache", "only-if-cached", "no-cache"]) {
    const url = "/cache/stale-" + mode + "?control=max-age%3D0";
    result["stale-" + mode] = [await read(url), await read(url, mode), await read(url)];
  }
  for (const control of ["no-store", "no-cache", "max-age=3600, must-revalidate"]) {
    const url =
      "/cache/control-" + encodeURIComponent(control) + "?control=" + encodeURIComponent(control);
    result["control-" + control] = [
      await read(url),
      await read(url, "force-cache"),
      await read(url),
    ];
  }
  result.vary = [];
  for (const value of ["a", "b", "a", "b"])
    result.vary.push(
      await read("/cache/vary?vary=x-flavor", "default", { headers: { "X-Flavor": value } }),
    );
  result.star = [
    await read("/cache/star?vary=*"),
    await read("/cache/star?vary=*"),
    await read("/cache/star?vary=*", "force-cache"),
    await read("/cache/star?vary=*", "only-if-cached"),
  ];
  result.emptyNoCache = await read("/cache/empty-no-cache", "no-cache");
  result.referrer = [
    await read("/cache/referrer"),
    await read("/cache/referrer", "default", { referrer: "" }),
  ];
  result.redirect = [
    await read("/cache/redirect?redirect=301"),
    await read("/cache/redirect?redirect=301"),
  ];
  result.invalidate = [
    await read("/cache/invalidate"),
    await read("/cache/invalidate", "reload", { headers: { "X-Response-Control": "no-store" } }),
    await read("/cache/invalidate"),
  ];
  result.unsafe = [
    await read("/cache/unsafe"),
    await read("/cache/unsafe", "default", { method: "POST" }),
    await read("/cache/unsafe"),
  ];
  const head = async (url) => {
    const response = await fetch(url, { method: "HEAD" });
    return {
      status: response.status,
      count: response.headers.get("x-count"),
      body: await response.text(),
    };
  };
  result.headFresh = [
    await read("/cache/head-fresh"),
    await head("/cache/head-fresh"),
    await read("/cache/head-fresh"),
  ];
  result.headMissing = [
    await head("/cache/head-missing"),
    await head("/cache/head-missing"),
    await read("/cache/head-missing"),
  ];
  result.varyReferrer = [
    await read("/cache/vary-referrer?vary=referer"),
    await read("/cache/vary-referrer?vary=referer", "default", { referrer: "" }),
    await read("/cache/vary-referrer?vary=referer", "default", { referrer: "" }),
  ];
  for (const status of [201, 202, 302, 307, 404, 500]) {
    const url = "/cache/status-" + status + "?status=" + status;
    result["status-" + status] = [await read(url), await read(url)];
  }
  result.redirect302 = [
    await read("/cache/redirect302?redirect=302"),
    await read("/cache/redirect302?redirect=302"),
  ];
  result.redirect307 = [
    await read("/cache/redirect307?redirect=307"),
    await read("/cache/redirect307?redirect=307"),
  ];
  return result;
})();
