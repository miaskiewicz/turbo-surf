(() => {
  const policy = document.featurePolicy || document.permissionsPolicy;
  return {
    secure: isSecureContext,
    isolated: crossOriginIsolated,
    sharedType: typeof SharedArrayBuffer,
    sharedBytes:
      typeof SharedArrayBuffer === "function" ? new SharedArrayBuffer(16).byteLength : null,
    allowedIsolation: policy ? policy.allowsFeature("cross-origin-isolated") : null,
    policyBrand: policy ? Object.prototype.toString.call(policy) : null,
    permissionPolicy: typeof document.permissionsPolicy,
    featurePolicy: typeof document.featurePolicy,
  };
})();
