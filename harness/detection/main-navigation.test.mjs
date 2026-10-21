import assert from "node:assert/strict";
import { test } from "node:test";
import { createMainNavigationTracker } from "./main-navigation.mjs";

const scope = { sessionId: "main-session", frameId: "main-frame" };
const request = (requestId, loaderId, extra = {}) => ({
  requestId,
  loaderId,
  frameId: scope.frameId,
  type: "Document",
  request: { url: "http://127.0.0.1/search" },
  ...extra,
});
const commit = (loaderId, extra = {}) => ({ id: scope.frameId, loaderId, ...extra });

test("main navigation ignores child frames, workers and unrelated subresource traffic", () => {
  const tracker = createMainNavigationTracker(scope);
  tracker.request(request("iframe", "child", { frameId: "child-frame" }), scope.sessionId);
  tracker.request(request("worker", "worker"), "worker-session");
  tracker.request(request("fetch", "old", { type: "Fetch" }), scope.sessionId);
  tracker.request(request("image", "old", { type: "Image" }), scope.sessionId);
  assert.deepEqual(tracker.snapshot(), {
    pendingNavigation: false,
    pendingNavigationRequest: null,
    navigationFailed: false,
    failedNavigationRequest: null,
  });
});

test("an old same-URL 200 loader cannot settle its provisional 429 replacement", () => {
  const tracker = createMainNavigationTracker(scope);
  tracker.commit(commit("old-200"), scope.sessionId);
  tracker.request(request("next-request", "next-429"), scope.sessionId);
  tracker.commit(commit("old-200"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, true);
  assert.equal(tracker.snapshot().pendingNavigationRequest.loaderId, "next-429");
  tracker.commit(commit("next-429"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, false);
  assert.equal(tracker.snapshot().navigationFailed, false);
});

test("main redirect hops retain the current provisional request until loader commit", () => {
  const tracker = createMainNavigationTracker(scope);
  tracker.request(request("redirect", "next"), scope.sessionId);
  tracker.request(
    request("redirect", "next", {
      redirectResponse: { status: 302 },
      request: { url: "http://127.0.0.1/challenge" },
    }),
    scope.sessionId,
  );
  const snapshot = tracker.snapshot();
  assert.equal(snapshot.pendingNavigationRequest.redirects, 1);
  assert.equal(snapshot.pendingNavigationRequest.url, "http://127.0.0.1/challenge");
  snapshot.pendingNavigationRequest.loaderId = "mutated-copy";
  assert.equal(tracker.snapshot().pendingNavigationRequest.loaderId, "next");
  tracker.commit(commit("next"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, false);
});

test("a superseded request failure cannot clear the newer main navigation", () => {
  const tracker = createMainNavigationTracker(scope);
  tracker.request(request("old-request", "old"), scope.sessionId);
  tracker.request(request("new-request", "new"), scope.sessionId);
  tracker.failed(
    { requestId: "old-request", errorText: "net::ERR_ABORTED", canceled: true },
    scope.sessionId,
  );
  assert.equal(tracker.snapshot().pendingNavigationRequest.requestId, "new-request");
  assert.equal(tracker.snapshot().navigationFailed, false);
  tracker.commit(commit("old"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, true);
  tracker.commit(commit("new"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, false);
});

test("main failures before or after commit remain explicit until a new navigation supersedes them", () => {
  for (const committed of [false, true]) {
    const tracker = createMainNavigationTracker(scope);
    tracker.request(request("failed-request", "failed"), scope.sessionId);
    if (committed) tracker.commit(commit("failed"), scope.sessionId);
    tracker.failed(
      { requestId: "failed-request", errorText: "net::ERR_CONNECTION_RESET" },
      scope.sessionId,
    );
    assert.equal(tracker.snapshot().pendingNavigation, false);
    assert.equal(tracker.snapshot().navigationFailed, true);
    assert.equal(tracker.snapshot().failedNavigationRequest.errorText, "net::ERR_CONNECTION_RESET");
    tracker.commit(commit("previous-document"), scope.sessionId);
    assert.equal(tracker.snapshot().navigationFailed, true);
    tracker.request(request("retry-request", "retry"), scope.sessionId);
    assert.equal(tracker.snapshot().navigationFailed, false);
    tracker.commit(commit("retry"), scope.sessionId);
    assert.equal(tracker.snapshot().pendingNavigation, false);
    tracker.failed({ requestId: "failed-request", errorText: "late old failure" }, scope.sessionId);
    assert.equal(tracker.snapshot().navigationFailed, false);
  }
});

test("only the scoped main-frame commit settles a request and changing owner clears it", () => {
  const tracker = createMainNavigationTracker();
  tracker.request(request("unconfigured", "next"), scope.sessionId);
  assert.equal(tracker.snapshot().pendingNavigation, false);
  tracker.setScope(scope);
  tracker.request(request("current", "next"), scope.sessionId);
  tracker.commit(commit("next", { parentId: "parent-frame" }), scope.sessionId);
  tracker.commit(commit("next"), "other-session");
  assert.equal(tracker.snapshot().pendingNavigation, true);
  tracker.setScope(scope);
  assert.equal(tracker.snapshot().pendingNavigation, true);
  tracker.setScope({ sessionId: "new-session", frameId: "new-frame" });
  assert.equal(tracker.snapshot().pendingNavigation, false);
  assert.equal(tracker.snapshot().navigationFailed, false);
});
