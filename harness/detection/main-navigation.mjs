// Only provisional main-document requests count here. Subresources and child
// frames can remain busy after a complete search document without postponing it.
export function createMainNavigationTracker(initialScope = {}) {
  let scope = { ...initialScope };
  let pending = null;
  let latest = null;
  let failure = null;
  const scoped = (sessionId) =>
    scope.sessionId != null && scope.frameId != null && sessionId === scope.sessionId;

  return {
    setScope(next) {
      if (next.sessionId !== scope.sessionId || next.frameId !== scope.frameId) {
        pending = null;
        latest = null;
        failure = null;
      }
      scope = { ...next };
    },
    request(event, sessionId) {
      if (
        !scoped(sessionId) ||
        event.frameId !== scope.frameId ||
        event.type !== "Document" ||
        typeof event.requestId !== "string" ||
        typeof event.loaderId !== "string" ||
        !event.loaderId
      )
        return;
      const redirects =
        latest?.requestId === event.requestId && event.redirectResponse ? latest.redirects + 1 : 0;
      pending = {
        requestId: event.requestId,
        loaderId: event.loaderId,
        url: event.request?.url ?? null,
        redirects,
      };
      latest = pending;
      failure = null;
    },
    commit(frame, sessionId) {
      if (!scoped(sessionId) || frame.id !== scope.frameId || frame.parentId) return;
      // A late event from the previous loader must not settle its replacement.
      if (pending && frame.loaderId === pending.loaderId) pending = null;
    },
    failed(event, sessionId) {
      if (!scoped(sessionId) || !latest || event.requestId !== latest.requestId) return;
      // Response bodies can fail after commit, while the partial DOM stays live.
      failure = {
        ...latest,
        errorText: event.errorText ?? null,
        canceled: event.canceled === true,
      };
      pending = null;
    },
    snapshot() {
      return {
        pendingNavigation: pending !== null,
        pendingNavigationRequest: pending && { ...pending },
        navigationFailed: failure !== null,
        failedNavigationRequest: failure && { ...failure },
      };
    },
  };
}
