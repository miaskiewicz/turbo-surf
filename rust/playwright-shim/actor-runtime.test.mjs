import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import test from "node:test";

const loader = new URL("../crates/turbo-surf-napi/index.js", import.meta.url).pathname;
try {
  createRequire(import.meta.url)(loader);
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

for (const asynchronous of [false, true]) {
  test(`actor ${asynchronous ? "async" : "sync"} read enters runtime without pumping`, () => {
    const child = spawnSync(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
      import assert from 'node:assert/strict';
      import {createRequire} from 'node:module';
      import {createServer} from 'node:http';
      const native=createRequire(import.meta.url)(${JSON.stringify(loader)});
      let requests=0;
      let response, requestArrived;
      const requested=new Promise(resolve=>{requestArrived=resolve});
      // Background IO may progress independently; keep the body pending during the read.
      const server=createServer((req,res)=>{requests++;response=res;requestArrived()});
      await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
      const base='http://127.0.0.1:'+server.address().port+'/';
      const id=await native.liveOpen('<body><iframe srcdoc="&lt;p&gt;child&lt;/p&gt;"></iframe>',base,'[]','');
      try {
        const read=script=>native.${asynchronous ? "liveReadEngineAsync" : "liveReadEngine"}(id,script);
        assert.deepEqual(JSON.parse(await read("globalThis.timerRan=false;setTimeout(()=>{timerRan=true},0);globalThis.fetchDone=false;globalThis.pendingRead=fetch('/read').then(r=>r.text()).then(value=>{fetchDone=true;return value});return [timerRan,document.querySelector('iframe').contentDocument.body.textContent]")),[false,'child']);
        await requested;
        await new Promise(resolve=>setTimeout(resolve,30));
        assert.deepEqual(JSON.parse(await read('return [timerRan,fetchDone]')),[false,false]);
        response.end('ready');
        assert.equal(await native.liveEvalEngineExpression(id,'pendingRead'),'ready');
        assert.equal(requests,1);
      } finally {
        native.liveClose(id);server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
      }
    `,
      ],
      { encoding: "utf8", timeout: 15000 },
    );
    assert.equal(child.signal, null, child.stderr);
    assert.equal(child.status, 0, child.stderr + child.stdout);
  });
}
