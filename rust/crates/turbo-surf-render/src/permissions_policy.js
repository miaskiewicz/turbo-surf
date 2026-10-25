const __policyRecords = __interfaceMap("PermissionsPolicy");
const __policyDocumentRecords = __interfaceMap("Document.PermissionsPolicy");
const __policyTypeError = TypeError,
  __policyString = String;
const __policyRecord = (receiver) => {
  if (!__policyRecords.has(receiver)) throw new __policyTypeError("Illegal invocation");
  return __policyRecords.get(receiver);
};
const __policyCreate = Object.create,
  __policyDefine = Object.defineProperty;
const __policyQuery = (receiver, name, target, action) => {
  const record = __policyRecord(receiver);
  if (record.frame) {
    const document = __baseApply(__baseOwner, record.owner, []);
    return ops.op_iframe_permissions_policy(
      document,
      record.owner,
      __baseApply(__baseURI, document, []),
      name,
      target,
      action,
    );
  }
  return ops.op_permissions_policy(record.owner, name, target, action);
};
const __policyDOMString = (value) => {
  if (typeof value === "symbol")
    throw new __policyTypeError("Cannot convert a Symbol value to a string");
  return __policyString(value);
};
class PermissionsPolicy {
  constructor() {
    throw new __policyTypeError("Illegal constructor");
  }
  allowsFeature(name, origin = undefined) {
    __policyRecord(this);
    if (!arguments.length) throw new __policyTypeError("Feature name is required");
    return __policyQuery(
      this,
      __policyDOMString(name),
      origin === undefined ? "" : __policyDOMString(origin),
      2,
    );
  }
  allowedFeatures() {
    return __policyQuery(this, "", "", 3);
  }
  features() {
    return __policyQuery(this, "", "", 0);
  }
  getAllowlistForFeature(name) {
    __policyRecord(this);
    if (!arguments.length) throw new __policyTypeError("Feature name is required");
    return __policyQuery(this, __policyDOMString(name), "", 1);
  }
}
Object.defineProperty(PermissionsPolicy.prototype, Symbol.toStringTag, {
  value: "PermissionsPolicy",
  configurable: true,
});
for (const name of ["allowsFeature", "allowedFeatures", "features", "getAllowlistForFeature"]) {
  const descriptor = Object.getOwnPropertyDescriptor(PermissionsPolicy.prototype, name);
  descriptor.value = __documentFunction(descriptor.value, name, descriptor.value.length);
  descriptor.enumerable = true;
  Object.defineProperty(PermissionsPolicy.prototype, name, descriptor);
}
globalThis.PermissionsPolicy = PermissionsPolicy;
globalThis.FeaturePolicy = PermissionsPolicy;
__defineDocumentAccessor("featurePolicy", (state, doc) => {
  if (!__policyDocumentRecords.has(doc)) {
    const policy = __policyCreate(PermissionsPolicy.prototype);
    __policyRecords.set(policy, { __proto__: null, owner: doc, frame: false });
    __policyDocumentRecords.set(doc, policy);
  }
  return __policyDocumentRecords.get(doc);
});
// Fullscreen and picture-in-picture availability is the document's own
// permissions policy; only the active document of a browsing context has one.
const __policyDocument = (receiver) => {
  let type;
  try {
    type = __documentApply(__documentType, receiver, []);
  } catch {
    type = 0;
  }
  if (type !== 9) throw new __policyTypeError("Illegal invocation");
  return receiver;
};
for (const [name, feature, lenient] of [
  ["fullscreenEnabled", "fullscreen", true],
  ["webkitFullscreenEnabled", "fullscreen", false],
  ["pictureInPictureEnabled", "picture-in-picture", false],
]) {
  const descriptor = {
    enumerable: true,
    configurable: true,
    get: __documentFunction(
      function () {
        return ops.op_document_feature_enabled(__policyDocument(this), feature);
      },
      `get ${name}`,
      0,
    ),
  };
  // [LegacyLenientSetter]: a branded no-op, so assignment never shadows the getter.
  if (lenient)
    descriptor.set = __documentFunction(
      function () {
        __policyDocument(this);
      },
      `set ${name}`,
      1,
    );
  __policyDefine(Document.prototype, name, descriptor);
}
