(async () => {
  const settings = globalThis.__coepRedirect;
  const cases = [
    ["source-open", settings.source],
    ["source-authorized", settings.source],
    ["cross-back", settings.cross],
    ["cross-open-back", settings.cross],
  ];
  const rows = [];
  for (const [id, origin] of cases) {
    try {
      const response = await fetch(origin + "/" + id, { mode: "no-cors", credentials: "include" });
      rows.push({ id, allowed: true, type: response.type, status: response.status });
    } catch (error) {
      rows.push({ id, allowed: false, name: error.name });
    }
  }
  return rows;
})();
