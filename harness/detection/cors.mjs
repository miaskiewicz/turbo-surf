// Two-origin localhost Fetch oracle. Uses independent jars and all four trace passes.
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const listen = (server) =>
  new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
const close = (server) => new Promise((done) => server.close(done));
export async function captureCors({
  binary,
  out,
  debuggerEnabled = false,
  interfaceScript,
  chromeBackend = "cdp",
}) {
  let origin;
  const requests = [];
  const target = createServer((request, response) => {
    requests.push({
      method: request.method,
      path: request.url,
      origin: request.headers.origin || null,
      cookie: !!request.headers.cookie,
      preflight: request.headers["access-control-request-headers"] || null,
    });
    const path = request.url.split("?")[0];
    if (path != "/denied" && path != "/no-cors")
      response.setHeader("Access-Control-Allow-Origin", path === "/include" ? origin : "*");
    if (path === "/include") response.setHeader("Access-Control-Allow-Credentials", "true");
    if (request.method === "OPTIONS") {
      response.setHeader("Access-Control-Allow-Methods", "PUT");
      if (path === "/preflight") response.setHeader("Access-Control-Allow-Headers", "x-token");
      response.writeHead(204);
      response.end();
      return;
    }
    response.setHeader("Content-Type", "text/plain");
    response.setHeader("X-Visible", "visible");
    response.setHeader("X-Secret", "secret");
    response.setHeader("Access-Control-Expose-Headers", "X-Visible");
    response.end(
      JSON.stringify({
        origin: request.headers.origin || null,
        method: request.method,
        cookie: !!request.headers.cookie,
        token: request.headers["x-token"] || null,
      }),
    );
  });
  const source = createServer((request, response) => {
    if (request.url === "/same") {
      response.setHeader("Content-Type", "text/plain");
      response.setHeader("X-Secret", "same");
      response.end("same");
      return;
    }
    response.setHeader("Content-Type", "text/html");
    response.setHeader("Set-Cookie", "cors-fixture=present; HttpOnly; Path=/; SameSite=Lax");
    const endpoint = `http://127.0.0.1:${target.address().port}`;
    response.end(`<!doctype html><body><script>
      (async()=>{
        const endpoint=${JSON.stringify(endpoint)},rows={};
        async function attempt(name,path,init={}){try{
          const response=await fetch(path.startsWith('/')?endpoint+path:path,init),body=await response.text();
          rows[name]={type:response.type,status:response.status,ok:response.ok,bodyUsed:response.bodyUsed,body:body.startsWith('{')?JSON.parse(body):body,visible:response.headers.get('x-visible'),secret:response.headers.get('x-secret')};
          if(rows[name].body?.origin)rows[name].body.origin='source';
        }catch(error){rows[name]={error:error.name};}}
        await attempt('same',location.origin+'/same');
        await attempt('cors','/allowed');
        await attempt('denied','/denied');
        await attempt('include','/include',{credentials:'include'});
        await attempt('includeWildcard','/allowed',{credentials:'include'});
        await attempt('opaque','/no-cors',{mode:'no-cors'});
        await attempt('preflight','/preflight',{method:'PUT',headers:{'X-Token':'value'}});
        await attempt('preflightDenied','/preflight-denied',{method:'PUT',headers:{'X-Token':'value'}});
        globalThis.__browserFixture=rows;document.body.setAttribute('data-browser-fixture',JSON.stringify(rows));
      })();
    </script></body>`);
  });
  try {
    await listen(target);
    await listen(source);
    origin = `http://127.0.0.1:${source.address().port}`;
    await new Promise((done, fail) => {
      const child = spawn(
        process.execPath,
        [
          join(here, "run.mjs"),
          "--live",
          "--chrome-backend",
          chromeBackend,
          "--url",
          origin + "/",
          "--no-interaction",
          "--wait",
          "500",
          "--ready-selector",
          "body[data-browser-fixture]",
          "--success-selector",
          "body[data-browser-fixture]",
          "--binary",
          binary,
          "--out",
          out,
          ...(debuggerEnabled ? ["--debugger"] : []),
          ...(interfaceScript ? ["--interface-script", interfaceScript] : []),
        ],
        { stdio: "inherit" },
      );
      child.on("error", fail);
      child.on("close", (code) =>
        code === 0 ? done() : fail(new Error(`Trace harness exited ${code}`)),
      );
    });
    const result = {};
    for (const pass of ["baseline", "interfaces"]) {
      const rows = ["chrome", "native"].map(
        (runtime) =>
          JSON.parse(readFileSync(join(out, `${runtime}-${pass}`, "trace.json"))).pages[0]
            .browserFixture,
      );
      if (rows.some((row) => !row || Object.keys(row).length !== 8))
        throw new Error("Incomplete CORS fixture");
      result[pass] = {
        chrome: rows[0],
        native: rows[1],
        differences: differences(rows[0], rows[1]),
      };
    }
    const record = {
      schemaVersion: 1,
      note: "Localhost cross-origin response/credential/preflight contracts, not a site detector conclusion.",
      result,
      requests,
    };
    writeFileSync(join(out, "cors-comparison.json"), JSON.stringify(record, null, 2), {
      mode: 0o600,
    });
    return record;
  } finally {
    await close(source);
    await close(target);
    writeFileSync(join(out, "fixture-requests.json"), JSON.stringify(requests, null, 2), {
      mode: 0o600,
    });
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2),
    flag = (name, fallback) => {
      const index = args.indexOf(name);
      return index < 0 ? fallback : args[index + 1];
    };
  if (args.includes("--run"))
    console.log(
      JSON.stringify(
        await captureCors({
          binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
          out: resolve(flag("--out", "/tmp/turbo-cors-parity")),
          debuggerEnabled: args.includes("--debugger"),
          interfaceScript: flag("--interface-script", undefined),
        }),
        null,
        2,
      ),
    );
}
