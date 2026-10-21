// Capture dictionary discovery and lifecycle in fresh installed Chrome, localhost only.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
import { startDictionaryServers, dictionaryCases } from "./compression-dictionary-server.mjs";

const output = resolve(
    process.argv[2] || "harness/detection/fixtures/compression-dictionary-chrome155.json",
  ),
  profile = mkdtempSync(join(tmpdir(), "turbo-dictionary-oracle-"));
const fixtures = await startDictionaryServers();
const wire = fixtures.wire;

let chrome, socket, connection;
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const kill = (signal) => {
  if (!chrome?.pid || chrome.exitCode !== null || chrome.signalCode !== null) return;
  try {
    process.kill(-chrome.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
};
try {
  const url = fixtures.primary + "/page?case=0";
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--ignore-certificate-errors-spki-list=" + fixtures.spki,
    ...chromeExtensionArgs({ url, disableExtensions: true }),
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
  const errors = [];
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion");
  const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await connection.call("Target.attachToTarget", { targetId, flatten: true });
  await connection.call("Runtime.enable", {}, sessionId);
  await connection.call("Page.enable", {}, sessionId);
  const evaluate = async (expression) => {
    const result = await connection.call(
      "Runtime.evaluate",
      { expression, returnByValue: true, awaitPromise: true },
      sessionId,
    );
    if (result.exceptionDetails)
      throw Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const result = [];
  for (let id = 0; id < dictionaryCases.length; id++) {
    await connection.call(
      "Page.navigate",
      { url: fixtures.primary + "/page?case=" + id },
      sessionId,
    );
    const deadline = Date.now() + 10000;
    while (
      !(await evaluate("typeof dictionaryEvents==='object' && document.readyState==='complete'"))
    ) {
      if (Date.now() > deadline) throw Error("Dictionary fixture did not load");
      await pause(25);
    }
    await pause(1800);
    const resource =
      (dictionaryCases[id].cross ? fixtures.peer : fixtures.primary) +
      (dictionaryCases[id].resource || "/resource-" + id) +
      "?case=" +
      id;
    let manual = null,
      head = null,
      priority = null;
    if (dictionaryCases[id].head) {
      const dictionary = fixtures.primary + "/dictionary?case=" + id;
      head = await evaluate(
        "(async()=>{const first=await fetch(" +
          JSON.stringify(dictionary) +
          ",{method:'HEAD'});const headBytes=(await first.text()).length;await fetch(" +
          JSON.stringify(resource + "&phase=before") +
          ").then(r=>r.text());const second=await fetch(" +
          JSON.stringify(dictionary) +
          ");const getBytes=(await second.text()).length;await fetch(" +
          JSON.stringify(resource + "&phase=after") +
          ").then(r=>r.text());await new Promise(resolve=>setTimeout(resolve,100));await fetch(" +
          JSON.stringify(resource + "&phase=after100") +
          ").then(r=>r.text());await new Promise(resolve=>setTimeout(resolve,400));await fetch(" +
          JSON.stringify(resource + "&phase=after500") +
          ").then(r=>r.text());return {headBytes,getBytes}})()",
      );
    } else if (dictionaryCases[id].prioritySpoof) {
      priority = await evaluate(
        "fetch(" +
          JSON.stringify(resource) +
          ",{headers:{Priority:'u=4, i'},dictionary_prefetch:true}).then(r=>({ok:r.ok})).catch(error=>({error:error.name,message:error.message}))",
      );
    } else if (dictionaryCases[id].manual) {
      const kind = dictionaryCases[id].manual;
      const hash = createHash("sha256")
        .update("authored reusable compression dictionary case " + id)
        .digest("base64");
      const supplied =
        kind === "id"
          ? { "Dictionary-ID": '"forged"' }
          : {
              "Available-Dictionary":
                kind === "matching"
                  ? ":" + hash + ":"
                  : kind === "mismatch"
                    ? ":AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:"
                    : "malformed",
            };
      manual = await evaluate(
        "(async()=>{const supplied=" +
          JSON.stringify(supplied) +
          ",headers=new Headers(supplied),request=new Request(" +
          JSON.stringify(resource) +
          ",{headers});const retained={headers:[headers.get('available-dictionary'),headers.get('dictionary-id')],request:[request.headers.get('available-dictionary'),request.headers.get('dictionary-id')]};try{const response=await fetch(request);await response.text();return {...retained,outcome:{ok:response.ok}};}catch(error){return {...retained,outcome:{error:error.name,message:error.message}}}})()",
      );
    } else
      await evaluate(
        "fetch(" +
          JSON.stringify(resource) +
          ",{cache:" +
          JSON.stringify(dictionaryCases[id].cache || "default") +
          "}).then(r=>r.text()).catch(error=>error.name)",
      );
    result.push({
      name: dictionaryCases[id].name,
      ...(manual ? { manual } : {}),
      ...(head ? { head } : {}),
      ...(priority ? { priority } : {}),
      ...(await evaluate(
        "({secure:isSecureContext,events:dictionaryEvents,support:document.createElement('link').relList.supports('compression-dictionary')})",
      )),
    });
  }
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    fixtureSha256: createHash("sha256")
      .update(readFileSync(new URL("./compression-dictionary-server.mjs", import.meta.url)))
      .digest("hex"),
    tlsTrust: "Only the checked fixture certificate SPKI is trusted for this localhost control",
    cases: dictionaryCases,
    collectorSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    errors,
    wire,
    result,
  };
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(control, null, 2)}\n`);
  if (errors.length)
    throw new Error(`Authored Chrome control reported ${errors.length} exception(s)`);
} finally {
  connection?.close();
  socket?.close();
  kill("SIGTERM");
  if (chrome && chrome.exitCode === null && chrome.signalCode === null)
    await Promise.race([new Promise((done) => chrome.once("exit", done)), pause(1000)]);
  if (chrome?.exitCode === null && chrome.signalCode === null) kill("SIGKILL");
  await fixtures.close();
  rmSync(profile, { recursive: true, force: true });
}
