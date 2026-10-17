(async () => {
  const rows = await Promise.all(globalThis.parityFetches);
  return rows.map((row) => ({
    ...row,
    referrer:
      typeof row.referrer === "string"
        ? row.referrer.replace(location.origin, "<origin>")
        : undefined,
  }));
})();
