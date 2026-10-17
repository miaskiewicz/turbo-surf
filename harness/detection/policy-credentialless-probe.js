/* eslint-disable unicorn/no-document-cookie -- This authored control measures the actual legacy cookie interface. */
(async () => {
  const sorted = (value) => value.split("; ").filter(Boolean).sort();
  document.cookie = "top=parent;Path=/";
  localStorage.clear();
  sessionStorage.clear();
  localStorage.setItem("partition", "parent");
  sessionStorage.setItem("partition", "parent");
  const result = [];
  for (const credentialless of [true, true, false, true]) {
    const frame = document.createElement("iframe");
    frame.credentialless = credentialless;
    const url = new URL(location.href);
    url.searchParams.set("child", "1");
    frame.src = url.href;
    const child = await new Promise((resolve) => {
      const listener = (event) => {
        if (
          event.source === frame.contentWindow &&
          event.data?.kind === "credentialless-partition"
        ) {
          removeEventListener("message", listener);
          resolve(event.data.result);
        }
      };
      addEventListener("message", listener);
      document.body.append(frame);
    });
    result.push({
      credentialless,
      child,
      parent: {
        cookie: sorted(document.cookie),
        local: localStorage.getItem("partition"),
        session: sessionStorage.getItem("partition"),
      },
    });
    frame.remove();
  }
  return result;
})();
