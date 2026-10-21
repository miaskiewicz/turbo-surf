(async () => {
  const target = new URL("/sandbox-module.js", location.href);
  target.search = location.search;
  try {
    const module = await import(target.href);
    return {
      ok: true,
      origin: module.observed.origin.replace(
        /http:\/\/(127\.0\.0\.1|localhost):\d+/g,
        (_, host) => `<${host}>`,
      ),
      cookie: module.observed.cookie,
    };
  } catch (error) {
    return { ok: false, error: error.name };
  }
})();
