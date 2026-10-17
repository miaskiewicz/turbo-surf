(() => {
  const attempt = (callback) => {
    try {
      return callback();
    } catch (error) {
      return { error: error.name };
    }
  };
  const headers = (request) => Array.from(request.headers);
  const request = new Request("/resource", {
    headers: {
      "X-Token": "value",
      Cookie: "secret",
      Origin: "https://spoof.test",
      "Sec-Fetch-Site": "none",
    },
  });
  const rows = { request: headers(request) };
  request.headers.set("Cookie", "override");
  request.headers.append("X-Token", "second");
  rows.mutated = headers(request);
  const opaque = new Request("/resource", {
    mode: "no-cors",
    headers: {
      "X-Token": "value",
      "Content-Type": "application/json",
      Accept: "text/html",
      "Accept-Language": "en-US",
    },
  });
  rows.noCors = headers(opaque);
  opaque.headers.set("Content-Type", "text/plain");
  opaque.headers.set("X-Token", "later");
  opaque.headers.append("Content-Type", "application/json");
  rows.noCorsMutated = headers(opaque);
  rows.invalid = [
    attempt(() => new Request("/resource", { mode: "no-cors", method: "PUT" })),
    attempt(() => new Headers([[Symbol(), "value"]])),
    attempt(() => new Headers([["key", Symbol()]])),
  ];
  return JSON.stringify(rows);
})();
