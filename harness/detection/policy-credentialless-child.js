/* eslint-disable unicorn/no-document-cookie -- This authored control measures the actual legacy cookie interface. */
(async () => {
  const sorted = (value) => value.split("; ").filter(Boolean).sort();
  const result = {
    before: {
      cookie: sorted(document.cookie),
      local: localStorage.getItem("partition"),
      session: sessionStorage.getItem("partition"),
    },
    navigationCookie: sorted(globalThis.__requestCookie),
  };
  document.cookie = "partition=child;Path=/";
  localStorage.setItem("partition", "child");
  sessionStorage.setItem("partition", "child");
  const url = new URL(location.href);
  url.searchParams.set("echo", "1");
  const response = await fetch(url.href);
  result.fetchCookie = sorted((await response.json()).cookie);
  parent.postMessage({ kind: "credentialless-partition", result }, "*");
})();
