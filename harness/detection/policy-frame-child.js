parent.postMessage(
  {
    kind: "policy-frame",
    isolated: crossOriginIsolated,
    shared: typeof SharedArrayBuffer,
    allowed: document.featurePolicy.allowedFeatures().sort(),
    allowlists: ["geolocation", "camera", "cross-origin-isolated", "autoplay"].map((name) => ({
      name,
      list: document.featurePolicy
        .getAllowlistForFeature(name)
        .map((value) => value.replace(location.origin, "<child-origin>")),
    })),
  },
  "*",
);
