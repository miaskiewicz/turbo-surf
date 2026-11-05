(async () => {
  "use strict";
  // connect-src for WebSocket and EventSource on a page whose meta policy is
  // `connect-src 'self'`: the same host:port over ws: and a different host.
  const port = location.port;
  const violations = [];
  document.addEventListener("securitypolicyviolation", (event) =>
    violations.push([
      event.blockedURI,
      event.violatedDirective,
      event.effectiveDirective,
      event.disposition,
    ]),
  );
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name, error.message];
    }
  };
  const socket = (url) =>
    new Promise((done) => {
      const made = run(() => new WebSocket(url));
      if (made[0] !== "ok") return done(made);
      const ws = made[1];
      const log = [["created", ws.readyState]];
      ws.onopen = () => {
        log.push(["open", ws.readyState]);
        ws.close();
      };
      // A policy-blocked socket reports error without a close event: settle
      // a little after it so a missing close is part of the observation.
      ws.onerror = () => {
        log.push(["error", ws.readyState]);
        setTimeout(() => done(log), 50);
      };
      ws.onclose = (event) => done(log.concat([["close", event.wasClean, event.code]]));
    });
  const source = (url) =>
    new Promise((done) => {
      const made = run(() => new EventSource(url));
      if (made[0] !== "ok") return done(made);
      const es = made[1];
      const log = [["created", es.readyState]];
      es.onmessage = (event) => {
        log.push(["message", event.data]);
        es.close();
        done(log);
      };
      es.onerror = () => {
        log.push(["error", es.readyState]);
        if (es.readyState === 2) done(log);
      };
    });
  const out = {
    sameOrigin: await socket(`ws://127.0.0.1:${port}/ws/echo`),
    otherHost: await socket(`ws://localhost:${port}/ws/echo`),
    sourceSame: await source("/sse/forever"),
    sourceOther: await source(`http://localhost:${port}/sse/forever`),
    violations,
  };
  // The server port differs per run.
  return JSON.parse(JSON.stringify(out).replaceAll(`:${port}`, ":<port>"));
})();
