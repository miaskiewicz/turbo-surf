// Independent offline Chrome lifecycle oracle over raw CDP, without a browser library.
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, existsSync, writeFileSync } from "node:fs";
const option = (name) =>
  process.argv.find((value) => value.startsWith(`--${name}=`))?.slice(name.length + 3);
const chromePath =
  option("chrome") || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const output = option("out") || "/tmp/turbo-window-events-cdp.json";
if (!existsSync(chromePath) || typeof WebSocket === "undefined") {
  console.log(
    JSON.stringify({ skipped: true, reason: "Installed Chrome and Node.js 22+ are required" }),
  );
  process.exit(0);
}
const directory = mkdtempSync("/tmp/turbo-event-cdp-");
const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--remote-debugging-port=0",
    `--user-data-dir=${directory}`,
    "--no-first-run",
    "--no-default-browser-check",
    "about:blank",
  ],
  { stdio: "ignore" },
);
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
try {
  for (let i = 0; i < 100 && !existsSync(`${directory}/DevToolsActivePort`); i++) await pause(30);
  const port = readFileSync(`${directory}/DevToolsActivePort`, "utf8").split("\n")[0];
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const socket = new WebSocket(tabs.find((tab) => tab.type === "page").webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  let sequence = 0;
  const pending = new Map();
  socket.onmessage = (message) => {
    const value = JSON.parse(message.data);
    if (value.id && pending.has(value.id)) {
      pending.get(value.id)(value);
      pending.delete(value.id);
    }
  };
  const call = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      const timeout = setTimeout(() => reject(new Error(`CDP ${method} timed out`)), 5000);
      pending.set(id, (value) => {
        clearTimeout(timeout);
        value.error ? reject(new Error(value.error.message)) : resolve(value);
      });
      socket.send(JSON.stringify({ id, method, params }));
    });
  await call("Page.enable");
  await call("Runtime.enable");
  const html = `<!doctype html><script>globalThis.records=[];globalThis.saved=[];const snapshot=e=>({type:e.type,target:e.target===document?'document':e.target===window?'window':null,currentTarget:e.currentTarget===window?'window':e.currentTarget===document?'document':null,phase:e.eventPhase,path:e.composedPath().length,trusted:e.isTrusted});for(const type of ['load','pageshow'])window.addEventListener(type,e=>{saved.push(e);records.push(snapshot(e));if(type==='pageshow')setTimeout(()=>{globalThis.completed=saved.map(snapshot);globalThis.redispatch=saved.map(event=>{try{return {accepted:new EventTarget().dispatchEvent(event)}}catch(error){return {error:error.name}}});globalThis.ready=true;},0);});</script><body>offline</body>`;
  await call("Page.navigate", { url: "data:text/html," + encodeURIComponent(html) });
  for (let i = 0; i < 100; i++) {
    await pause(20);
    const state = await call("Runtime.evaluate", {
      expression: "globalThis.ready===true",
      returnByValue: true,
    });
    if (state.result.result.value) break;
  }
  const result = await call("Runtime.evaluate", {
    expression: "JSON.stringify({ready,records,completed,redispatch})",
    returnByValue: true,
  });
  const proof = {
    browserVersion: version.Browser,
    backend: "raw Chrome DevTools Protocol, no Playwright/Patchright",
    ...JSON.parse(result.result.result.value),
  };
  writeFileSync(output, JSON.stringify(proof, null, 2) + "\n");
  console.log(JSON.stringify(proof));
  socket.close();
} finally {
  chrome.kill("SIGTERM");
}
