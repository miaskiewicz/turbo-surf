(() => {
  const policy = document.featurePolicy;
  const methods = ["allowsFeature", "allowedFeatures", "features", "getAllowlistForFeature"];
  const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, "featurePolicy");
  const shapes = methods.map((name) => {
    const member = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(policy), name);
    const errors = [];
    for (const receiver of [{}, null, new Proxy(policy, {})]) {
      try {
        member.value.call(receiver, "geolocation");
        errors.push("accepted");
      } catch (error) {
        errors.push(error.name);
      }
    }
    return {
      name,
      length: member.value.length,
      enumerable: member.enumerable,
      configurable: member.configurable,
      writable: member.writable,
      prototype: Object.hasOwn(member.value, "prototype"),
      errors,
    };
  });
  let wasmShared = null,
    wasmClone = null;
  try {
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
    wasmShared = {
      brand: Object.prototype.toString.call(memory.buffer),
      bytes: memory.buffer.byteLength,
    };
    try {
      wasmClone = Object.prototype.toString.call(structuredClone(memory.buffer));
    } catch (error) {
      wasmClone = error.name;
    }
  } catch (error) {
    wasmShared = error.name;
  }
  return {
    brand: Object.prototype.toString.call(policy),
    constructor: policy.constructor.name,
    globals: {
      PermissionsPolicy: typeof PermissionsPolicy,
      FeaturePolicy: typeof FeaturePolicy,
      alias: PermissionsPolicy === FeaturePolicy,
    },
    features: policy.features().sort(),
    defaults: policy
      .features()
      .sort()
      .map((name) => ({
        name,
        list: policy
          .getAllowlistForFeature(name)
          .map((value) => value.replace(location.origin, "<origin>")),
      })),
    allowed: policy.allowedFeatures().sort(),
    geolocation: policy
      .getAllowlistForFeature("geolocation")
      .map((value) => value.replace(location.origin, "<origin>")),
    unknown: policy.getAllowlistForFeature("unknown-feature"),
    shapes,
    descriptor: {
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
      setter: typeof descriptor.set,
    },
    wasmShared,
    wasmClone,
  };
})();
