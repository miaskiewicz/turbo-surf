(() => {
  const policy = document.featurePolicy;
  const features = [
    "geolocation",
    "camera",
    "cross-origin-isolated",
    "autoplay",
    "unknown-feature",
  ];
  return features.map((name) => ({
    name,
    allowed: policy.allowsFeature(name),
    own: policy.allowsFeature(name, location.origin),
    foreign: policy.allowsFeature(name, "https://foreign.test"),
    invalid: policy.allowsFeature(name, "not-an-origin"),
    list: policy
      .getAllowlistForFeature(name)
      .map((value) => value.replace(location.origin, "<origin>")),
  }));
})();
