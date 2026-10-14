const __iframePolicyRecords = __interfaceMap("HTMLIFrameElement.PermissionsPolicy");
__policyDefine(HTMLIFrameElement.prototype, "featurePolicy", {
  enumerable: true,
  configurable: true,
  get: __documentFunction(
    function () {
      if (!__htmlPolicyElement(this, "iframe")) throw new __policyTypeError("Illegal invocation");
      if (!__iframePolicyRecords.has(this)) {
        const policy = __policyCreate(PermissionsPolicy.prototype);
        __policyRecords.set(policy, { __proto__: null, owner: this, frame: true });
        __iframePolicyRecords.set(this, policy);
      }
      return __iframePolicyRecords.get(this);
    },
    "get featurePolicy",
    0,
  ),
});
