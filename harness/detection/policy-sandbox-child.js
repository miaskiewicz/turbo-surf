(() => {
  const error = (fn) => {
    try {
      fn();
      return null;
    } catch (value) {
      return value.name;
    }
  };
  parent.postMessage(
    {
      kind: "policy-frame",
      origin,
      locationOrigin: location.origin.replace(
        /http:\/\/(127\.0\.0\.1|localhost):\d+/g,
        (_, host) => `<${host}>`,
      ),
      secure: isSecureContext,
      isolated: crossOriginIsolated,
      cookie: error(() => {
        // oxlint-disable-next-line unicorn/no-document-cookie -- The control measures this exact native accessor.
        document.cookie = "sandbox=1;path=/";
        void document.cookie;
      }),
      storage: error(() => localStorage.setItem("sandbox", "1")),
    },
    "*",
  );
})();
