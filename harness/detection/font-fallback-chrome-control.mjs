// Fresh independent platform-font evidence for original-unit fallback controls.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import { cdpConnection } from "./cdp-connection.mjs";

const { values } = parseArgs({ options: { out: { type: "string" }, chrome: { type: "string" } } });
if (!values.out)
  throw Error("Pass --out /absolute/control.json; optionally --chrome /absolute/Chrome");
const server = createServer();
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const port = server.address().port;
await new Promise((done) => server.close(done));
const profile = mkdtempSync(`${tmpdir()}/turbo-font-control-`);
const executable =
  values.chrome ||
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const chrome = spawn(executable, [
  "--headless=new",
  "--no-first-run",
  "--disable-extensions",
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${profile}`,
  "about:blank",
]);
let connection;
let socket;
let failure;
chrome.on("error", (error) => {
  failure = error;
});
try {
  let version;
  for (let attempt = 0; attempt < 300; attempt++) {
    if (failure) throw failure;
    if (chrome.exitCode !== null) throw Error(`Chrome exited ${chrome.exitCode}`);
    try {
      version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      break;
    } catch {
      await new Promise((done) => setTimeout(done, 100));
    }
  }
  if (!version) throw Error("Chrome remote debugging startup timed out");
  socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  connection = cdpConnection(socket, () => {});
  const { targetInfos } = await connection.call("Target.getTargets");
  const { sessionId } = await connection.call("Target.attachToTarget", {
    targetId: targetInfos.find((target) => target.type === "page").targetId,
    flatten: true,
  });
  await connection.call("DOM.enable", {}, sessionId);
  await connection.call("CSS.enable", {}, sessionId);
  const cases = [];
  for (const family of ["Times New Roman", "Arial", "Courier New"])
    for (const [name, text] of [
      ["lone", "\ud800"],
      ["replacement", "\ufffd"],
      ["mixedOriginal", "a\ud835\udc00\ud800b\udcffc"],
      ["mixedProjection", "a\ud835\udc00\ufffdb\ufffdc"],
    ])
      cases.push({ family, name, text, size: 64 });
  await connection.call(
    "Runtime.evaluate",
    {
      expression: `document.body.innerHTML='';(${JSON.stringify(cases)}).forEach((row,index)=>{const element=document.createElement('div');element.id='font'+index;element.style.cssText='font-family:'+row.family+';font-size:'+row.size+'px';element.textContent=row.text;document.body.append(element)});document.body.offsetHeight`,
    },
    sessionId,
  );
  const { root } = await connection.call("DOM.getDocument", {}, sessionId);
  const result = [];
  for (const [index, row] of cases.entries()) {
    const { nodeId } = await connection.call(
      "DOM.querySelector",
      { nodeId: root.nodeId, selector: `#font${index}` },
      sessionId,
    );
    const { fonts } = await connection.call("CSS.getPlatformFontsForNode", { nodeId }, sessionId);
    result.push({ ...row, fonts });
  }
  const sourceSha256 = createHash("sha256")
    .update(readFileSync(import.meta.filename))
    .digest("hex");
  writeFileSync(
    resolve(values.out),
    `${JSON.stringify({ browser: version.Browser, v8: version["V8-Version"], sourceSha256, result }, null, 2)}\n`,
  );
} finally {
  connection?.close();
  socket?.close();
  if (chrome.exitCode === null && !failure) {
    chrome.kill();
    await new Promise((done) => chrome.once("exit", done));
  }
  rmSync(profile, { recursive: true, force: true });
}
