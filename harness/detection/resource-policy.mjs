// Authored localhost resources for paired SRI/CORP controls; no external requests.
import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { captureApiShape } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const script = "globalThis.integrityScript=(globalThis.integrityScript||0)+1;";
const moduleSource =
  "import './dep.mjs';globalThis.integrityModule=(globalThis.integrityModule||0)+1;";
const style = "body { color: rgb(12, 34, 56); }";
const bytes = Buffer.from([114, 97, 119, 255, 98, 121, 116, 101, 115]);
const integrity = (body) => `sha256-${createHash("sha256").update(body).digest("base64")}`;

export async function startResourceFixture() {
  const servers = [];
  let cross;
  const serve = (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    let body = script;
    let type = "text/javascript";
    const policy = {
      "/corp-origin.js": "same-origin",
      "/corp-site.js": "same-site",
      "/corp-cross.js": "cross-origin",
      "/corp-invalid.js": "Same-Origin",
    }[path];
    if (policy) response.setHeader("Cross-Origin-Resource-Policy", policy);
    if (path === "/corp-duplicate.js") {
      response.setHeader("Cross-Origin-Resource-Policy", ["same-origin", "same-site"]);
    }
    if (path === "/data") {
      type = "application/octet-stream";
      body = bytes;
    } else if (["/entry.mjs", "/blocked.mjs"].includes(path)) {
      body = moduleSource;
    } else if (path === "/dep.mjs") {
      body = "globalThis.integrityDependency=true;";
    } else if (path === "/style.css") {
      type = "text/css";
      body = style;
    } else if (path === "/blocked.css") {
      type = "text/css";
      body = "body { background-color: rgb(99, 88, 77); }";
    } else if (path === "/frame") {
      type = "text/html";
      body =
        "<!doctype html><body><script>globalThis.integrityErrors=0;</script><script src='/script.js' integrity='sha256-YQ==' onerror='integrityErrors++'></script></body>";
    } else if (path === "/") {
      type = "text/html";
      body = `<!doctype html><head>
        <meta name=fixture-cross content='${cross}'>
        <meta name=fixture-data-integrity content='${integrity(bytes)}'>
        <script>globalThis.resourceEvents=[];function recordResource(id,event){resourceEvents.push([id,event.type,event.isTrusted])}</script>
        <link id=valid-style rel=stylesheet href=/style.css integrity='${integrity(style)}' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'>
        <link id=blocked-style rel=stylesheet href=/blocked.css integrity='sha256-YQ==' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'>
        <script id=valid-script src=/script.js integrity='${integrity(script)}' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'></script>
        <script id=blocked-script src=/blocked.js integrity='sha256-YQ==' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'></script>
        <script id=valid-module type=module src=/entry.mjs integrity='${integrity(moduleSource)}' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'></script>
        <script id=blocked-module type=module src=/blocked.mjs integrity='sha256-YQ==' onload='recordResource(this.id,event)' onerror='recordResource(this.id,event)'></script>
        </head><body><iframe src=/frame></iframe></body>`;
    }
    response.setHeader("Access-Control-Allow-Origin", "*");
    response.setHeader("Content-Type", type);
    response.end(body);
  };
  const listen = async () => {
    const server = createServer(serve);
    servers.push(server);
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    return `http://127.0.0.1:${server.address().port}`;
  };
  try {
    cross = await listen();
    const origin = await listen();
    return {
      origin,
      cross,
      close: async () => {
        await Promise.all(servers.map((server) => new Promise((done) => server.close(done))));
      },
    };
  } catch (error) {
    await Promise.all(servers.map((server) => new Promise((done) => server.close(done))));
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name, fallback) => {
    const index = args.indexOf(name);
    return index < 0 ? fallback : args[index + 1];
  };
  if (!args.includes("--run"))
    console.log("Use --run --binary PATH for the paired localhost resource-policy audit.");
  else {
    const fixture = await startResourceFixture();
    try {
      const result = await captureApiShape({
        binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
        out: resolve(flag("--out", join(here, "artifacts", `resources-${Date.now()}`))),
        collector: join(here, "resource-policy-probe.js"),
        fixtureUrl: fixture.origin,
      });
      console.log(JSON.stringify({ engine: result.build.engine, differences: result.differences }));
    } finally {
      await fixture.close();
    }
  }
}
