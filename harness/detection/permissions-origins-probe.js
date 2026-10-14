(() => {
  const policy = document.featurePolicy;
  const origins = [
    location.origin,
    "https://example.test",
    "https://a.example.test",
    "https://b.a.example.test",
    "https://notexample.test",
    "http://a.example.test",
    "https://a.example.test:443",
    "https://a.example.test:8443",
    "https://foreign.test/path",
    "null",
  ];
  return {
    featureOrder: policy.features(),
    allowedOrder: policy.allowedFeatures(),
    list: policy
      .getAllowlistForFeature("geolocation")
      .map((value) => value.replace(location.origin, "<origin>")),
    allows: origins.map((value) => policy.allowsFeature("geolocation", value)),
    camera: policy.allowsFeature("camera"),
  };
})();
