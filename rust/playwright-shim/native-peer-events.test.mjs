import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let native, BrowserContext;
try {
  ({ BrowserContext } = await import("./index.mjs"));
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
const detection = new URL("../../harness/detection/", import.meta.url);
const source = readFileSync(new URL("native-peer-events-probe.js", detection), "utf8");
const control = JSON.parse(
  readFileSync(new URL("fixtures/native-peer-events-chrome155.json", detection)),
);

async function fixture(run) {
  let started, closed, heldResponse;
  const holdStarted = new Promise((resolve) => {
    started = resolve;
  });
  const holdClosed = new Promise((resolve) => {
    closed = resolve;
  });
  const server = createServer((_request, response) => {
    response.setHeader("Connection", "close");
    response.setHeader("Content-Type", "text/html");
    if (_request.url === "/delayed") {
      setTimeout(() => response.end("delayed"), 100);
      return;
    }
    if (_request.url === "/hold") {
      response.setHeader("Cache-Control", "no-store");
      response.write("pending");
      heldResponse = response;
      response.on("close", closed);
      started();
      return;
    }
    response.end("<!doctype html><body>Native peer events</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}/`;
  const context = native.requestStateNew();
  const pages = [];
  const open = async (path) => {
    const state = native.requestStateNew(context);
    const url = base + path;
    const response = JSON.parse(
      await native.fetchWithCookies(
        url,
        "[]",
        null,
        null,
        null,
        false,
        state,
        JSON.stringify({ destination: "document" }),
      ),
    );
    const session = await native.liveOpen(
      response.html,
      url,
      response.cookies,
      "",
      state,
      JSON.stringify({
        status: response.status,
        referrer: response.referrer,
        headers: response.headers,
      }),
    );
    const page = { state, session };
    pages.push(page);
    return page;
  };
  const evaluate = (page, expression) => native.liveEval(page.session, expression);
  const invoke = async (page, operation) =>
    JSON.parse(
      await evaluate(
        page,
        `(async()=>{globalThis.__RESULT=JSON.stringify(await(${source.trim().replace(/;$/, "")} )(${JSON.stringify(operation)}));})()`,
      ),
    );
  const peer = async (page, count) => {
    const deadline = Date.now() + 3000;
    for (;;) {
      const result = await invoke(page, {});
      if (result.events.length >= count) return result;
      assert.ok(
        Date.now() < deadline,
        `native peer missing storage tasks: ${result.events.length}/${count}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
  };
  try {
    await run({
      open,
      invoke,
      peer,
      evaluate,
      base,
      holdStarted,
      holdClosed,
      holdIsOpen: () => heldResponse && !heldResponse.writableEnded && !heldResponse.destroyed,
    });
  } finally {
    for (const page of pages) {
      native.liveClose(page.session);
      native.requestStateClose(page.state);
    }
    native.requestStateClose(context);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("native workers match Chrome trusted storage tasks, UTF16, source exclusion and tab namespaces", async () => {
  assert.equal(control.browser, "Chrome/155.0.8059.79");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  for (const [field, path] of [
    ["probeSha256", "native-peer-events-probe.js"],
    ["collectorSha256", "native-peer-events-chrome.mjs"],
  ])
    assert.equal(
      createHash("sha256")
        .update(readFileSync(new URL(path, detection)))
        .digest("hex"),
      control[field],
    );
  await fixture(async ({ open, invoke, peer }) => {
    const a = await open("writer#source"),
      b = await open("peer#target");
    await invoke(a, { init: true });
    await invoke(b, { init: true, tamper: true });
    const result = {};
    result.firstSource = await invoke(a, {
      steps: [
        ["local", "set", "key\ud800", "value\udfff"],
        ["local", "set", "key\ud800", "value\udfff"],
        ["local", "delete", "missing"],
        ["session", "set", "secret", "alpha"],
      ],
    });
    result.firstPeer = await peer(b, 2);
    result.burstSource = await invoke(a, {
      steps: [
        ["local", "set", "burst", "one"],
        ["local", "set", "burst", "two"],
        ["local", "delete", "key\ud800"],
        ["local", "clear"],
        ["local", "clear"],
      ],
    });
    result.burstPeer = await peer(b, 10);
    const c = await open("late");
    result.latePeer = await invoke(c, { init: true });
    result.reverseSource = await invoke(b, {
      steps: [
        ["local", "set", "back", "returned"],
        ["session", "set", "secret", "beta"],
      ],
    });
    result.reversePeer = await peer(a, 2);
    result.lateAfter = await peer(c, 2);
    assert.deepEqual(result, control.result);
  });
});

test("all peer inbox budgets are checked before storage commit and recover after delivery", async () => {
  await fixture(async ({ open, evaluate }) => {
    const a = await open("writer"),
      b = await open("blocked"),
      c = await open("idle");
    await evaluate(
      b,
      'globalThis.received=0;addEventListener("storage",()=>received++);globalThis.__RESULT="ready"',
    );
    await evaluate(
      c,
      'globalThis.received=0;addEventListener("storage",()=>received++);globalThis.__RESULT="ready"',
    );
    await evaluate(
      b,
      `(async()=>{const frame=document.createElement('iframe');const loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;globalThis.__RESULT='frame-ready'})()`,
    );
    const blockedB = evaluate(
      b,
      "{const start=performance.now();while(performance.now()-start<1500){};globalThis.__RESULT='released'}",
    );
    const blockedC = evaluate(
      c,
      "{const start=performance.now();while(performance.now()-start<3000){};globalThis.__RESULT='released'}",
    );
    await new Promise((resolve) => setTimeout(resolve, 30));
    const result = JSON.parse(
      await evaluate(
        a,
        `(async()=>{
      const values=['a'.repeat(1024*1024),'b'.repeat(1024*1024)];let last=null,error='none',count=0;
      for(;count<50;count++){const next=values[count%2];try{localStorage.setItem('budget',next);last=next;}catch(e){error=e.name;break;}}
      globalThis.__RESULT=JSON.stringify({atomic:localStorage.getItem('budget')===last,error,count});
    })()`,
      ),
    );
    assert.equal(result.atomic, true);
    assert.equal(result.error, "RangeError");
    assert.ok(result.count > 0 && result.count < 50);
    await blockedB;
    await evaluate(b, "globalThis.__RESULT='drained'");
    assert.equal(
      await evaluate(
        b,
        "sessionStorage.setItem('accounting','x'.repeat(2*1024*1024));globalThis.__RESULT='per-queue-recovered'",
      ),
      "per-queue-recovered",
    );
    await blockedC;
    await evaluate(c, "globalThis.__RESULT='drained'");
    assert.equal(
      await evaluate(
        a,
        "localStorage.setItem('budget','recovered');globalThis.__RESULT=localStorage.getItem('budget')",
      ),
      "recovered",
    );
  });
});

test("a pending peer Promise receives a storage task without another peer RPC", async () => {
  await fixture(async ({ open, evaluate }) => {
    const source = await open("writer"),
      peer = await open("waiting");
    const received = native.liveEvalExpression(
      peer.session,
      `new Promise(resolve=>addEventListener('storage',event=>resolve(event.key),{once:true}))`,
    );
    await new Promise((resolve) => setTimeout(resolve, 30));
    await evaluate(source, `localStorage.setItem('wake','value');globalThis.__RESULT='written'`);
    assert.equal(
      await Promise.race([
        received,
        new Promise((_, reject) =>
          setTimeout(() => reject(new Error("parked peer was not woken")), 3000),
        ),
      ]),
      control.pendingWake,
    );
  });
});

test("an idle native worker runs its storage handler without a receiver RPC", async () => {
  await fixture(async ({ open, evaluate }) => {
    const source = await open("writer"),
      peer = await open("idle");
    await evaluate(
      peer,
      "addEventListener('storage',event=>{if(event.key==='idle-write')localStorage.setItem('idle-ack',event.newValue)});globalThis.__RESULT='ready'",
    );
    await evaluate(
      source,
      "localStorage.setItem('idle-write','value');globalThis.__RESULT='written'",
    );
    const deadline = Date.now() + 3000;
    for (;;) {
      const ack = await evaluate(source, "globalThis.__RESULT=localStorage.getItem('idle-ack')");
      if (ack === control.idleAck) break;
      assert.ok(Date.now() < deadline, "idle peer handler did not execute");
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
  });
});

test("shim page.evaluate awaits a storage Promise from another page", async () => {
  await fixture(async ({ base }) => {
    const context = new BrowserContext();
    try {
      const source = await context.newPage(),
        peer = await context.newPage();
      await source.goto(base, { waitUntil: "networkidle" });
      await peer.goto(base, { waitUntil: "networkidle" });
      const waiting = peer.evaluate(
        () =>
          new Promise((resolve) =>
            addEventListener("storage", (event) => resolve(event.key), { once: true }),
          ),
      );
      await new Promise((resolve) => setTimeout(resolve, 30));
      await source.evaluate(() => localStorage.setItem("wake", "value"));
      assert.equal(await waiting, control.pendingWake);
    } finally {
      await context.close();
    }
  });
});

test("closing an idle receiver cancels storage-handler stalled IO without another receiver RPC", async () => {
  await fixture(async ({ base, holdStarted, holdClosed, holdIsOpen }) => {
    const modulePath = createRequire(import.meta.url).resolve("../crates/turbo-surf-napi/index.js");
    const code = `import {createRequire} from 'node:module';
      const native=createRequire(import.meta.url)(${JSON.stringify(modulePath)});
      const parent=native.requestStateNew(),a=native.requestStateNew(parent),b=native.requestStateNew(parent);
      const source=await native.liveOpen('<body>',${JSON.stringify(base + "source")},'[]','',a);
      const peer=await native.liveOpen('<body>',${JSON.stringify(base + "peer")},'[]','',b);
      await native.liveEval(peer, "addEventListener('storage',async()=>{await(await fetch('/hold')).text()});globalThis.__RESULT='ready'");
      const command=expected=>new Promise((resolve,reject)=>process.once('message',value=>value===expected?resolve():reject(new Error('unexpected close-test command'))));
      const start=command('start');process.send('ready');await start;
      const close=command('close');
      await native.liveEval(source,"localStorage.setItem('start','value');globalThis.__RESULT='sent'");
      await close;
      native.liveClose(peer);native.liveClose(source);
      native.requestStateClose(a);native.requestStateClose(b);native.requestStateClose(parent);
      process.stdout.write('closed');process.disconnect();`;
    const child = spawn(process.execPath, ["--input-type=module", "-e", code], {
      stdio: ["ignore", "pipe", "pipe", "ipc"],
    });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      if (stderr.length < 8192) stderr += chunk;
    });
    const exited = new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
    const ready = new Promise((resolve, reject) => {
      child.once("message", (message) => {
        if (message === "ready") resolve();
        else reject(new Error("unexpected close-test readiness message"));
      });
    });
    const prematureExit = exited.then((result) => {
      throw new Error(
        `idle receiver exited before stalled IO: ${JSON.stringify(result)} ${stdout} ${stderr}`,
      );
    });
    let timer;
    const deadline = (phase, milliseconds) =>
      new Promise((_, reject) => {
        timer = setTimeout(() => {
          child.kill("SIGKILL");
          reject(new Error(`idle close ${phase} timed out: ${stdout} ${stderr}`));
        }, milliseconds);
      });
    try {
      // Whole-system font discovery is cold process startup; cancellation keeps
      // its original five-second bound after the receiver is actually ready.
      await Promise.race([ready, prematureExit, deadline("startup", 30000)]);
      clearTimeout(timer);
      const operationDeadline = deadline("operation", 5000);
      child.send("start");
      await Promise.race([holdStarted, prematureExit, operationDeadline]);
      assert.equal(holdIsOpen(), true, "receiver fetch must still be stalled before close");
      child.send("close");
      assert.deepEqual(
        await Promise.race([exited, operationDeadline]),
        { code: 0, signal: null },
        stderr,
      );
      await Promise.race([holdClosed, operationDeadline]);
      assert.equal(stdout, "closed");
    } finally {
      clearTimeout(timer);
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      await exited.catch(() => {});
    }
  });
});

test("idle onstorage async IO continues without a receiver RPC", async () => {
  await fixture(async ({ open, evaluate }) => {
    const source = await open("source"),
      peer = await open("peer");
    await evaluate(
      peer,
      "onstorage=async event=>{if(event.key==='idle-delayed')localStorage.setItem('delayed-ack',await(await fetch('/delayed')).text())};globalThis.__RESULT='ready'",
    );
    await evaluate(
      source,
      "localStorage.setItem('idle-delayed','value');globalThis.__RESULT='sent'",
    );
    const deadline = Date.now() + 3000;
    for (;;) {
      const ack = await evaluate(source, "globalThis.__RESULT=localStorage.getItem('delayed-ack')");
      if (ack === control.delayedAck) break;
      assert.ok(Date.now() < deadline, "idle async handler continuation did not run");
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
  });
});
