(async () => {
  const cross = document.body.getAttribute("data-cross");
  const source = new URL(location.href);
  const normalize = (value) =>
    value === null ? null : value.replace(source.origin, "SOURCE").replace(cross, "CROSS");
  const request = async (url, init) => normalize((await (await fetch(url, init)).json()).referer);
  const rows = { defaults: [await request("/echo"), await request(cross + "/echo")] };
  rows.policies = {};
  for (const policy of [
    "no-referrer",
    "no-referrer-when-downgrade",
    "same-origin",
    "origin",
    "strict-origin",
    "origin-when-cross-origin",
    "strict-origin-when-cross-origin",
    "unsafe-url",
  ]) {
    rows.policies[policy] = [
      await request("/echo", { referrerPolicy: policy }),
      await request(cross + "/echo", { referrerPolicy: policy }),
    ];
  }
  rows.redirects = {};
  for (const policy of ["no-referrer", "origin", "unsafe-url", "invalid"]) {
    rows.redirects[policy] = await request("/redirect?policy=" + policy, {
      referrerPolicy: "unsafe-url",
    });
  }
  rows.referrer = {};
  for (const referrer of [
    "",
    "about:client",
    source.origin + "/authored?token=value#fragment",
    cross + "/foreign",
  ]) {
    try {
      rows.referrer[normalize(referrer)] = await request("/echo", { referrer });
    } catch (error) {
      rows.referrer[normalize(referrer)] = error.name;
    }
  }
  return rows;
})();
