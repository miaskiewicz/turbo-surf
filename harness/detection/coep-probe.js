(async () => {
  const policies = globalThis.__coepPolicies;
  const rows = [];
  for (const item of policies.cases) {
    for (const mode of ["no-cors", "cors"]) {
      const id = item.id + ":" + mode;
      try {
        const response = await fetch(policies.cross + "/echo?case=" + item.id + "&mode=" + mode, {
          mode,
          credentials: "include",
        });
        rows.push({ id, allowed: true, type: response.type, status: response.status });
      } catch (error) {
        rows.push({ id, allowed: false, name: error.name });
      }
    }
  }
  return rows;
})();
