import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { superviseChild, superviseTestChild } from "./child-supervision.mjs";

test("child supervision preserves exit status and bounded output", async () => {
  const streamed = { stdout: "", stderr: "" };
  const result = await superviseChild(
    process.execPath,
    ["-e", 'process.stdout.write("123456789");process.stderr.write("error");process.exitCode=7'],
    {
      timeoutMs: 5000,
      maxOutputBytes: 5,
      onOutput: (name, bytes) => {
        streamed[name] += bytes;
      },
    },
  );
  assert.equal(result.code, 7);
  assert.equal(result.signal, null);
  assert.equal(result.timedOut, false);
  assert.equal(result.aborted, false);
  assert.equal(result.error, null);
  assert.equal(result.stdout, "56789");
  assert.equal(result.stderr, "error");
  assert.deepEqual(streamed, { stdout: "123456789", stderr: "error" });
});

test("an output callback failure remains failed after the collector finishes cleanup", async () => {
  const directory = mkdtempSync(join(tmpdir(), "turbo-output-failure-fixture-"));
  const evidence = join(directory, "trace.json");
  try {
    const result = await superviseChild(
      process.execPath,
      [
        "-e",
        `const {writeFileSync}=require('node:fs');
        process.on('SIGTERM',()=>{
          writeFileSync(process.argv[1],JSON.stringify({captureComplete:false,cleanupFinished:true}));
          process.exit(0);
        });
        process.stdout.write('ready');setInterval(()=>{},1000);`,
        evidence,
      ],
      {
        timeoutMs: 5000,
        killGraceMs: 100,
        onOutput: () => {
          throw new Error("fixture log destination failed");
        },
      },
    );
    assert.match(result.error, /Output callback failed: Error: fixture log destination failed/);
    assert.equal(result.timedOut, false);
    assert.equal(result.aborted, false);
    assert.equal(result.code, 0);
    assert.deepEqual(JSON.parse(readFileSync(evidence, "utf8")), {
      captureComplete: false,
      cleanupFinished: true,
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("child supervision reports startup errors and does not launch an already aborted command", async () => {
  const result = await superviseChild(join(tmpdir(), "missing-supervised-executable"), [], {
    timeoutMs: 5000,
  });
  assert.match(result.error, /ENOENT/);
  assert.equal(result.timedOut, false);
  assert.equal(result.aborted, false);
  const controller = new AbortController();
  controller.abort();
  const cancelled = await superviseChild(process.execPath, ["-e", "throw Error('must not run')"], {
    signal: controller.signal,
    timeoutMs: 5000,
  });
  assert.equal(cancelled.aborted, true);
  assert.equal(cancelled.error, null);
  assert.equal(cancelled.stderr, "");
  assert.equal(cancelled.code, null);
});

test("out-of-range deadlines cannot wrap into a one-millisecond timeout", () => {
  for (const timeoutMs of [0, -1, 1.5, 2 ** 31, Infinity])
    assert.throws(() => superviseChild(process.execPath, [], { timeoutMs }), TypeError);
  assert.throws(
    () => superviseChild(process.execPath, [], { timeoutMs: 1000, killGraceMs: 2 ** 31 }),
    TypeError,
  );
});

async function stalledCollector(t, reason) {
  const directory = mkdtempSync(join(tmpdir(), "turbo-supervision-fixture-"));
  const evidence = join(directory, "trace.json");
  const controller = new AbortController();
  let descendant;
  let accepted = false;
  const server = createServer((request) => {
    descendant = Number(new URL(request.url, "http://fixture.test").searchParams.get("pid"));
    accepted = true;
    if (reason === "abort") controller.abort();
    // Deliberately leave the request pending: cleanup must interrupt real work.
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const fixture = `
      const {spawn}=require('node:child_process');
      const {request}=require('node:http');
      const {writeFileSync}=require('node:fs');
      const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});
      const pending=request(process.argv[1]+'/?pid='+child.pid);pending.on('error',()=>{});pending.end();
      process.on('SIGTERM',()=>{
        child.once('exit',()=>{
          writeFileSync(process.argv[2],JSON.stringify({captureComplete:false,descendantExited:true}));
          pending.destroy();process.exit(0);
        });
        child.kill('SIGTERM');
      });
    `;
    const result = await superviseChild(
      process.execPath,
      ["-e", fixture, `http://127.0.0.1:${server.address().port}`, evidence],
      { signal: controller.signal, timeoutMs: 1000, killGraceMs: 1000 },
    );
    assert.equal(accepted, true, "the child reached the stalled localhost request");
    assert.equal(result.code, 0, result.stderr);
    assert.equal(result.timedOut, reason === "timeout");
    assert.equal(result.aborted, reason === "abort");
    assert.equal(result.error, null);
    assert.deepEqual(JSON.parse(readFileSync(evidence, "utf8")), {
      captureComplete: false,
      descendantExited: true,
    });
    assert.throws(() => process.kill(descendant, 0), { code: "ESRCH" });
  } finally {
    if (descendant) {
      try {
        process.kill(descendant, "SIGKILL");
      } catch {}
    }
    server.closeAllConnections();
    if (server.listening) await new Promise((done) => server.close(done));
    rmSync(directory, { recursive: true, force: true });
  }
  t.diagnostic("The incomplete trace survived termination until explicit fixture cleanup.");
}

test(
  "a deadline remains failed after graceful cleanup of a stalled collector and detached child",
  {
    skip: process.platform === "win32",
  },
  async (t) => stalledCollector(t, "timeout"),
);

test(
  "test-context abort waits for a stalled collector's detached child and incomplete trace",
  {
    skip: process.platform === "win32",
  },
  async (t) => stalledCollector(t, "abort"),
);

test("a child ignoring TERM is killed after the cleanup grace", async () => {
  const result = await superviseChild(
    process.execPath,
    ["-e", "process.on('SIGTERM',()=>{});setInterval(()=>{},1000)"],
    {
      timeoutMs: 1000,
      killGraceMs: 50,
    },
  );
  assert.equal(result.timedOut, true);
  assert.equal(result.code, null);
  assert.equal(result.signal, process.platform === "win32" ? "SIGTERM" : "SIGKILL");
});

test(
  "a retired collector leader cannot strand a same-group helper that ignores TERM",
  { skip: process.platform === "win32" },
  async () => {
    let descendant;
    let reported = "";
    try {
      const fixture = `
        const {spawn}=require('node:child_process');
        const child=spawn(process.execPath,['-e',
          "process.on('SIGTERM',()=>{});process.stdout.write('ready');setInterval(()=>{},1000)"
        ],{stdio:['ignore','pipe','ignore']});
        child.stdout.once('data',()=>{
          child.stdout.destroy();
          process.stdout.write(JSON.stringify({pid:child.pid}));
        });
        process.on('SIGTERM',()=>process.exit(0));setInterval(()=>{},1000);
      `;
      // Retire the leader once it has reported its helper (turbo-surf#44): a 1 s deadline
      // raced two Node startups on a busy host. The abort takes the deadline's stop() path;
      // timeoutMs is only a generous cap.
      const retire = new AbortController();
      const result = await superviseChild(process.execPath, ["-e", fixture], {
        signal: retire.signal,
        timeoutMs: 60_000,
        killGraceMs: 100,
        onOutput: (name, bytes) => {
          if (name !== "stdout") return;
          reported += bytes;
          if (reported.endsWith("}")) retire.abort();
        },
      });
      descendant = JSON.parse(reported).pid;
      assert.equal(result.code, 0, result.stderr);
      assert.equal(result.aborted, true);
      assert.equal(result.timedOut, false);
      assert.equal(result.error, null);
      assert.ok(descendant > 0, "the resistant descendant acknowledged readiness");
      const deadline = Date.now() + 1000;
      while (Date.now() < deadline) {
        try {
          process.kill(descendant, 0);
        } catch (error) {
          if (error.code === "ESRCH") return;
          throw error;
        }
        await new Promise((done) => setTimeout(done, 10));
      }
      assert.fail("The retired collector left its TERM-resistant helper alive");
    } finally {
      if (descendant) {
        try {
          process.kill(descendant, "SIGKILL");
        } catch {}
      }
    }
  },
);

test(
  "an actual node:test timeout awaits owned cleanup before starting the following case",
  { skip: process.platform === "win32" },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-test-timeout-fixture-"));
    const moduleUrl = new URL("./child-supervision.mjs", import.meta.url).href;
    try {
      for (const withHook of [false, true]) {
        const name = withHook ? "with-hook" : "without-hook";
        const cleanup = join(directory, `${name}-cleanup.json`);
        const bodyFinished = join(directory, `${name}-body-finished`);
        const owned = join(directory, `${name}-owned.json`);
        const marker = join(directory, `${name}-next.json`);
        const fixture = join(directory, `${name}.test.mjs`);
        const collector = `
          const {spawn}=require('node:child_process');
          const {writeFileSync}=require('node:fs');
          const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});
          writeFileSync(process.argv[2],JSON.stringify({pid:child.pid}));
          process.on('SIGTERM',()=>setTimeout(()=>{
            child.once('exit',()=>{
              writeFileSync(process.argv[1],JSON.stringify({captureComplete:false,descendantExited:true}));
              process.exit(0);
            });
            child.kill('SIGTERM');
          },150));
          setInterval(()=>{},1000);
        `;
        writeFileSync(
          fixture,
          `import {test} from 'node:test';
          import assert from 'node:assert/strict';
          import {existsSync,readFileSync,writeFileSync} from 'node:fs';
          import {superviseChild,superviseTestChild} from ${JSON.stringify(moduleUrl)};
          test('intentional outer timeout',{timeout:1000},async t=>{
            const args=['-e',${JSON.stringify(collector)},${JSON.stringify(cleanup)},${JSON.stringify(owned)}];
            const options={timeoutMs:5000,killGraceMs:500,signal:t.signal};
            const pending=${withHook ? "superviseTestChild(t,process.execPath,args,options)" : "superviseChild(process.execPath,args,options)"};
            try {await pending;} finally {writeFileSync(${JSON.stringify(bodyFinished)},'finished');}
          });
          test('following case checks cleanup',()=>{
            const state={cleanupPresent:existsSync(${JSON.stringify(cleanup)}),bodyFinished:existsSync(${JSON.stringify(bodyFinished)})};
            if(state.cleanupPresent){
              const {pid}=JSON.parse(readFileSync(${JSON.stringify(owned)},'utf8'));
              try {process.kill(pid,0);state.descendantGone=false;}
              catch(error){if(error.code!=='ESRCH')throw error;state.descendantGone=true;}
            }else state.descendantGone=false;
            writeFileSync(${JSON.stringify(marker)},JSON.stringify(state));
            assert.deepEqual(state,{cleanupPresent:true,bodyFinished:true,descendantGone:true});
          });`,
        );
        const env = { ...process.env };
        // The subprocess must own a real runner, rather than Node's recursive
        // runner guard returning successfully without executing the fixture.
        delete env.NODE_TEST_CONTEXT;
        const result = await superviseTestChild(
          t,
          process.execPath,
          ["--test", "--test-concurrency=1", fixture],
          { timeoutMs: 10000, killGraceMs: 100, spawnOptions: { env } },
        );
        assert.equal(
          result.code,
          1,
          `the intentional outer timeout must remain failed: ${result.stdout}${result.stderr}`,
        );
        assert.equal(result.timedOut, false, result.stdout + result.stderr);
        assert.equal(result.error, null);
        assert.match(result.stdout + result.stderr, /test timed out after 1000ms/);
        const state = JSON.parse(readFileSync(marker, "utf8"));
        assert.deepEqual(state, {
          cleanupPresent: withHook,
          bodyFinished: withHook,
          descendantGone: withHook,
        });
        assert.deepEqual(JSON.parse(readFileSync(cleanup, "utf8")), {
          captureComplete: false,
          descendantExited: true,
        });
      }
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);
