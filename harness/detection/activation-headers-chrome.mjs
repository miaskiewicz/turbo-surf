// Real localhost navigation headers from a fresh normal installed Chrome.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const output = process.argv[2];
if (!output) throw new Error("Usage: node activation-headers-chrome.mjs output.json");
const cases = [
  "script-submit",
  "script-request-submit",
  "untrusted-dispatch",
  "trusted-click",
  "trusted-enter",
  "trusted-handler-submit",
  "trusted-click-location",
  "trusted-keydown-location",
  "trusted-pointerdown-submit",
  "canceled-pointerdown-submit",
  "expired-activation",
  "automatic-retry",
  "trusted-cross-origin-form",
  "trusted-cross-origin-redirect",
];
const requests = [],
  errors = [];
const profile = mkdtempSync(join(tmpdir(), "turbo-activation-oracle-"));
const handler = (request, response) => {
  const requestUrl = new URL(request.url, "http://localhost");
  const [phase, name] = requestUrl.pathname.split("/").slice(1);
  response.setHeader("Cache-Control", "no-store");
  if (cases.includes(name))
    requests.push({
      name,
      phase,
      origin: request.socket.localPort === server.address().port ? "primary" : "secondary",
      method: request.method,
      secFetchUser: request.headers["sec-fetch-user"] || null,
      ...(phase === "next"
        ? {
            activation: {
              isActive: requestUrl.searchParams.get("active") === "true",
              hasBeenActive: requestUrl.searchParams.get("sticky") === "true",
            },
          }
        : {}),
    });
  if (phase === "submit" || phase === "next" || phase === "retry") {
    response.writeHead(302, {
      Location: `${name === "trusted-cross-origin-redirect" && phase === "submit" ? `http://127.0.0.1:${secondary.address().port}` : ""}/${phase !== "retry" ? "destination" : "final"}/${name}`,
    });
    response.end();
    return;
  }
  response.setHeader("Content-Type", "text/html");
  if (phase === "start") {
    response.end(
      `<!doctype html><form id=f action="${name === "trusted-cross-origin-form" ? `http://127.0.0.1:${secondary.address().port}` : ""}/submit/${name}"><input id=q name=q><button id=b>Go</button></form>`,
    );
  } else if (phase === "destination") {
    response.end(
      `<!doctype html><form id=f action="/newdoc/${name}"><button>Go</button></form>${name === "automatic-retry" || name.includes("-location") || name.includes("cross-origin") ? `<script>location.replace('/retry/${name}')</script>` : ""}`,
    );
  } else response.end("<!doctype html><body>done</body>");
};
const server = createServer(handler);
const secondary = createServer(handler);
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
let chrome, socket, connection;
const kill = (signal) => {
  if (chrome?.pid && chrome.exitCode === null && chrome.signalCode === null) {
    try {
      process.kill(-chrome.pid, signal);
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  }
};
try {
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  await new Promise((done, fail) => {
    secondary.once("error", fail);
    secondary.listen(0, "127.0.0.1", done);
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    ...chromeExtensionArgs({ url: origin, disableExtensions: true }),
    "about:blank",
  ];
  chrome = spawn(
    process.env.TURBO_SURF_CHROME_PATH ||
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    flags,
    { detached: true, stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "",
    spawnError;
  chrome.once("error", (error) => {
    spawnError = error;
  });
  chrome.stderr.on("data", (chunk) => {
    if (stderr.length < 1048576) stderr += String(chunk);
  });
  const deadline = Date.now() + 30000;
  let endpoint;
  while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null || Date.now() > deadline) throw new Error("Chrome startup failed");
    await pause(50);
  }
  socket = new WebSocket(endpoint);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion");
  const results = [];
  let activationShape;
  for (const name of cases) {
    const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await connection.call("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    const call = (method, params = {}) => connection.call(method, params, sessionId);
    const evaluate = async (expression) => {
      const result = await call("Runtime.evaluate", {
        expression,
        returnByValue: true,
        awaitPromise: true,
        userGesture: false,
      });
      if (result.exceptionDetails) throw new Error("Chrome activation probe evaluation failed");
      return result.result.value;
    };
    const waitFor = async (test) => {
      const until = Date.now() + 10000;
      while (!(await test())) {
        if (Date.now() > until) throw new Error(`Timed out: ${name}`);
        await pause(20);
      }
    };
    await call("Runtime.enable");
    await call("Page.enable");
    await call("Page.navigate", { url: `${origin}/start/${name}` });
    await waitFor(() =>
      evaluate("document.readyState==='complete'&&!!document.getElementById('q')"),
    );
    if (!activationShape)
      activationShape = await evaluate(`(async()=>{
      const descriptor=(owner,name)=>{const d=Object.getOwnPropertyDescriptor(owner,name);return d?{configurable:d.configurable,enumerable:d.enumerable,...('writable'in d?{writable:d.writable}:{}),...('value'in d?{valueType:typeof d.value,name:d.value?.name,length:d.value?.length}:{}),...(d.get?{getter:{name:d.get.name,length:d.get.length}}:{}),...(d.set?{setter:{name:d.set.name,length:d.set.length}}:{})}:null};
      const failure=fn=>{try{fn();return null}catch(error){return {name:error.name,message:error.message}}};
      const p=UserActivation.prototype;const getter=Object.getOwnPropertyDescriptor(p,'isActive').get;
      const frame=document.createElement('iframe');frame.srcdoc='<!doctype html>';await new Promise(done=>{frame.onload=done;document.body.append(frame)});
      const foreign=frame.contentWindow;const foreignGetter=Object.getOwnPropertyDescriptor(foreign.UserActivation.prototype,'isActive').get;
      const result={global:descriptor(globalThis,'UserActivation'),prototype:descriptor(UserActivation,'prototype'),ownKeys:Reflect.ownKeys(p).map(String),members:Object.fromEntries(['hasBeenActive','isActive','constructor'].map(name=>[name,descriptor(p,name)])),navigator:descriptor(Navigator.prototype,'userActivation'),sameObject:navigator.userActivation===navigator.userActivation,illegalConstructor:failure(()=>new UserActivation()),forgedGetter:failure(()=>getter.call({})),crossRealm:{parentGetterOnChild:getter.call(foreign.navigator.userActivation),childGetterOnParent:foreignGetter.call(navigator.userActivation)}};
      frame.remove();return result;
    })()`);
    const initialActivation = await evaluate(
      "({isActive:navigator.userActivation.isActive,hasBeenActive:navigator.userActivation.hasBeenActive})",
    );
    const click = async (selector) => {
      const point = await evaluate(
        `(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`,
      );
      await call("Input.dispatchMouseEvent", {
        type: "mousePressed",
        ...point,
        button: "left",
        clickCount: 1,
      });
      await call("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        ...point,
        button: "left",
        clickCount: 1,
      });
    };
    let expiredActivation = null;
    if (name === "script-submit") await evaluate("f.submit()");
    else if (name === "script-request-submit") await evaluate("f.requestSubmit()");
    else if (name === "untrusted-dispatch")
      await evaluate(
        "q.dispatchEvent(new MouseEvent('mousedown',{bubbles:true}));f.requestSubmit()",
      );
    else if (name === "trusted-enter") {
      await click("#q");
      await call("Input.dispatchKeyEvent", {
        type: "keyDown",
        key: "Enter",
        code: "Enter",
        windowsVirtualKeyCode: 13,
        text: "\r",
      });
      await call("Input.dispatchKeyEvent", {
        type: "keyUp",
        key: "Enter",
        code: "Enter",
        windowsVirtualKeyCode: 13,
      });
    } else if (name === "trusted-click-location" || name === "trusted-keydown-location") {
      const target = `/next/${name}`;
      const expression = `${JSON.stringify(target)}+'?active='+navigator.userActivation.isActive+'&sticky='+navigator.userActivation.hasBeenActive`;
      await evaluate(
        `${name === "trusted-click-location" ? "b" : "q"}.addEventListener('${name === "trusted-click-location" ? "click" : "keydown"}',event=>{event.preventDefault();${name === "trusted-click-location" ? `location.assign(${expression})` : `location.href=${expression}`}})`,
      );
      await click(name === "trusted-click-location" ? "#b" : "#q");
      if (name === "trusted-keydown-location") {
        await call("Input.dispatchKeyEvent", {
          type: "keyDown",
          key: "a",
          code: "KeyA",
          windowsVirtualKeyCode: 65,
          text: "a",
        });
        await call("Input.dispatchKeyEvent", {
          type: "keyUp",
          key: "a",
          code: "KeyA",
          windowsVirtualKeyCode: 65,
        });
      }
    } else if (name === "trusted-handler-submit") {
      await evaluate("b.addEventListener('click',event=>{event.preventDefault();f.submit()})");
      await click("#b");
    } else if (name === "trusted-pointerdown-submit" || name === "canceled-pointerdown-submit") {
      await evaluate(
        `b.addEventListener('pointerdown',event=>{${name === "canceled-pointerdown-submit" ? "event.preventDefault();" : ""}f.submit()})`,
      );
      await click("#b");
    } else if (name === "expired-activation") {
      await click("#q");
      await pause(5200);
      expiredActivation = await evaluate(
        "({isActive:navigator.userActivation.isActive,hasBeenActive:navigator.userActivation.hasBeenActive})",
      );
      if (expiredActivation.isActive)
        throw new Error("Activation did not expire after real elapsed wait");
      await evaluate("f.submit()");
    } else await click("#b");
    const terminal =
      name === "automatic-retry" || name.includes("-location") || name.includes("cross-origin")
        ? "final"
        : "destination";
    await waitFor(() =>
      Promise.resolve(requests.some((row) => row.name === name && row.phase === terminal)),
    );
    await waitFor(() =>
      evaluate(
        `location.pathname===${JSON.stringify(`/${terminal}/${name}`)}&&document.readyState==='complete'`,
      ),
    );
    const destinationActivation = await evaluate(
      "({isActive:navigator.userActivation.isActive,hasBeenActive:navigator.userActivation.hasBeenActive})",
    );
    if (terminal === "destination") {
      await evaluate("f.submit()");
      await waitFor(() =>
        Promise.resolve(requests.some((row) => row.name === name && row.phase === "newdoc")),
      );
    }
    results.push({
      name,
      initialActivation,
      destinationActivation,
      ...(expiredActivation ? { expiredActivation } : {}),
      requests: requests.filter((row) => row.name === name && row.phase !== "start"),
    });
    await connection.call("Target.closeTarget", { targetId });
  }
  const fixture = {
    browser: version.product,
    v8: version.jsVersion,
    sourceSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    freshProfile: true,
    activationShape,
    realElapsedExpiryMs: 5200,
    errors,
    results,
  };
  mkdirSync(dirname(resolve(output)), { recursive: true });
  writeFileSync(output, `${JSON.stringify(fixture, null, 2)}\n`);
  if (errors.length) throw new Error(`Chrome reported ${errors.length} exceptions`);
} finally {
  connection?.close();
  socket?.close();
  kill("SIGTERM");
  if (chrome && chrome.exitCode === null && chrome.signalCode === null)
    await Promise.race([new Promise((done) => chrome.once("exit", done)), pause(1000)]);
  kill("SIGKILL");
  secondary.closeAllConnections();
  await new Promise((done) => secondary.close(done));
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
