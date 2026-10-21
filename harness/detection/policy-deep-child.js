(async () => {
  const url = new URL(location.href);
  const depth = Number(url.searchParams.get("depth"));
  const item = JSON.parse(url.searchParams.get("config"));
  const features = ["geolocation", "camera", "cross-origin-isolated", "autoplay"];
  const result = {
    depth,
    isolated: crossOriginIsolated,
    allowed: features.map((name) => document.featurePolicy.allowsFeature(name)),
    lists: features.map((name) =>
      document.featurePolicy
        .getAllowlistForFeature(name)
        .map((value) =>
          value.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, (_, host) => `<${host}>`),
        ),
    ),
    permission: (await navigator.permissions.query({ name: "geolocation" })).state,
  };
  if (depth === 1) {
    const child = document.createElement("iframe");
    if (item.innerAllow !== null) child.setAttribute("allow", item.innerAllow);
    url.searchParams.set("depth", "2");
    if (item.innerCross) url.hostname = url.hostname === "localhost" ? "127.0.0.1" : "localhost";
    result.child = await new Promise((resolve) => {
      const listener = (event) => {
        if (event.source !== child.contentWindow || event.data?.kind !== "policy-deep") return;
        window.removeEventListener("message", listener);
        resolve(event.data.result);
      };
      window.addEventListener("message", listener);
      child.src = url.href;
      document.body.append(child);
    });
    child.remove();
  }
  parent.postMessage({ kind: "policy-deep", result }, "*");
})().catch((error) =>
  parent.postMessage(
    { kind: "policy-deep", result: { failure: error.name, detail: error.message } },
    "*",
  ),
);
