// Topics and Private State Token queries ([SecureContext] Document members).
// Chrome 156 rejects document.browsingTopics() unconditionally once its options
// convert. A top-frame origin associates with at most two issuers in the browsing
// profile; this engine never issues or redeems tokens, so an associated query
// resolves false and a third issuer is refused as Chrome's network service does.
if (ops.op_is_secure_context()) {
  const privacyTypeError = TypeError,
    privacyException = DOMException,
    privacyString = String,
    privacyReject = (error) => __storageReject(error);
  const privacyFailure = (member) => `Failed to execute '${member}' on 'Document': `;
  const privacyDocument = (receiver, member) => {
    let type;
    try {
      type = __documentApply(__documentType, receiver, []);
    } catch {
      type = 0;
    }
    if (type !== 9) throw new privacyTypeError(privacyFailure(member) + "Illegal invocation");
  };
  // The receiver's browsing context, if it still has one, and the realm it settles in.
  const privacyContext = (receiver) => {
    let state = null;
    try {
      state = __documentState(receiver);
    } catch {}
    const active =
      !!state && !!state.window && !state.inactive && ops.op_document_has_active_window(receiver);
    return { active, realm: (state && state.storageRealm) || __storageRealm };
  };
  const privacyIssuer = (member, args) => {
    const prefix = privacyFailure(member);
    if (args.length < 1)
      throw new privacyTypeError(prefix + "1 argument required, but only 0 present.");
    const issuer = args[0];
    if (typeof issuer === "symbol")
      throw new privacyTypeError(prefix + "Cannot convert a Symbol value to a string");
    const origin = ops.op_private_token_issuer(privacyString(issuer));
    if (origin === null)
      throw new privacyTypeError(
        prefix +
          member +
          ': Private Token issuer origins must be both HTTP(S) and secure ("potentially trustworthy").',
      );
    return origin;
  };
  const privacyQuery = (receiver, member, args, associate) => {
    let context, origin;
    try {
      privacyDocument(receiver, member);
      origin = privacyIssuer(member, args);
      context = privacyContext(receiver);
      if (!context.active)
        throw new privacyException(
          privacyFailure(member) +
            member +
            ": Cannot execute in documents lacking top-frame origins.",
          "InvalidStateError",
        );
    } catch (error) {
      return privacyReject(error);
    }
    const status = ops.op_private_token_query(receiver, origin, associate);
    if (status === 1)
      return privacyReject(
        new privacyException(
          privacyFailure(member) +
            member +
            ": Cannot execute in documents without secure, HTTP(S), top-frame origins.",
          "NotAllowedError",
        ),
      );
    if (status === 2)
      return context.realm.reject(
        new (context.realm.exception || privacyException)(
          `Failed to retrieve ${member} response.`,
          "OperationError",
        ),
      );
    return context.realm.resolve(false);
  };
  const privacyMembers = {
    browsingTopics(options = undefined) {
      try {
        privacyDocument(this, "browsingTopics");
        if (options !== undefined && options !== null) {
          if (typeof options !== "object" && typeof options !== "function")
            throw new privacyTypeError(
              privacyFailure("browsingTopics") +
                "The provided value is not of type 'BrowsingTopicsOptions'.",
            );
          options.skipObservation;
        }
        throw new privacyException(
          privacyFailure("browsingTopics") +
            "document.browsingTopics() is deprecated and has been removed.",
          "NotSupportedError",
        );
      } catch (error) {
        return privacyReject(error);
      }
    },
    hasPrivateToken() {
      return privacyQuery(this, "hasPrivateToken", arguments, true);
    },
    hasRedemptionRecord() {
      return privacyQuery(this, "hasRedemptionRecord", arguments, false);
    },
  };
  for (const [name, length] of [
    ["browsingTopics", 0],
    ["hasPrivateToken", 1],
    ["hasRedemptionRecord", 1],
  ])
    Object.defineProperty(Document.prototype, name, {
      value: __documentFunction(privacyMembers[name], name, length),
      writable: true,
      enumerable: true,
      configurable: true,
    });
}
