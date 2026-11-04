(async () => {
  // Reading a used or locked body: the rejection type and message per reader
  // method, for Response and Request, plus clone() and the new Request(request)
  // copy of a used body.
  const out = {};
  const methods = ["arrayBuffer", "bytes", "text", "json", "blob", "formData"];
  const make = (Kind) =>
    Kind === "Request"
      ? new Request("http://127.0.0.1/", {
          method: "POST",
          body: "a=1",
          headers: { "content-type": "application/x-www-form-urlencoded" },
        })
      : new Response("a=1", { headers: { "content-type": "application/x-www-form-urlencoded" } });
  const settle = async (run) => {
    try {
      const value = await run();
      return ["ok", typeof value];
    } catch (error) {
      return [error?.name, error?.message];
    }
  };
  const states = {
    used: async (target) => {
      await target.text();
    },
    locked: async (target) => {
      target.body.getReader();
    },
    disturbed: async (target) => {
      const reader = target.body.getReader();
      await reader.read();
      reader.releaseLock();
    },
  };
  for (const Kind of ["Response", "Request"]) {
    for (const [state, prepare] of Object.entries(states)) {
      const row = {};
      for (const method of methods) {
        const target = make(Kind);
        await prepare(target);
        row[method] = [target.bodyUsed, await settle(() => target[method]())];
      }
      const target = make(Kind);
      await prepare(target);
      row.clone = await settle(() => target.clone());
      if (Kind === "Request") {
        const copy = make(Kind);
        await prepare(copy);
        row.construct = await settle(() => new Request(copy));
      }
      out[`${Kind}/${state}`] = row;
    }
  }
  return JSON.stringify(out);
})();
