// Paired localhost classic-script exception and resource-event control.
import { createServer } from "node:http";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { captureApiShape } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const fixture = `<!doctype html><body>
<script>
globalThis.errors=[];globalThis.resources=[];
addEventListener('error',event=>errors.push({
  trusted:event.isTrusted,identity:event.error===globalThis.originalError,
  ownRealm:event.error instanceof Error,source:event.filename,
  located:event.lineno>0&&event.colno>0,message:event.message
}));
globalThis.originalError=new Error('inline failure');throw originalError;
</script>
<script src='/throw.js' onload="resources.push(['throw','load',event.isTrusted])" onerror="resources.push(['throw','error',event.isTrusted])"></script>
<script src='/syntax.js' onload="resources.push(['syntax','load',event.isTrusted])" onerror="resources.push(['syntax','error',event.isTrusted])"></script>
<script>globalThis.continued=true;</script></body>`;

export async function startScriptErrorFixture() {
  let cross;
  const servers = [];
  const serve = (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    const script = {
      "/throw.js": "globalThis.originalError=new Error('downloaded failure');throw originalError;",
      "/syntax.js": "const = invalid;",
    }[path];
    response.setHeader("Content-Type", script ? "text/javascript" : "text/html");
    response.setHeader("Access-Control-Allow-Origin", "*");
    const document = fixture.replace(
      "<script>globalThis.continued=true;",
      `<script src='${cross}/throw.js' onload="resources.push(['muted','load',event.isTrusted])"></script><script src='${cross}/throw.js' crossorigin=anonymous onload="resources.push(['cors','load',event.isTrusted])"></script><script>globalThis.continued=true;`,
    );
    response.end(script || (path === "/" ? `${document}<iframe src='/frame'></iframe>` : document));
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
  const close = () =>
    Promise.all(servers.map((server) => new Promise((done) => server.close(done))));
  try {
    cross = await listen();
    return { origin: await listen(), cross, close };
  } catch (error) {
    await close();
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
    console.log("Use --run --binary PATH for paired localhost script error controls.");
  else {
    const server = await startScriptErrorFixture();
    try {
      const result = await captureApiShape({
        binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
        out: resolve(flag("--out", join(here, "artifacts", `script-errors-${Date.now()}`))),
        collector: join(here, "classic-script-errors-probe.js"),
        fixtureUrl: server.origin,
      });
      console.log(JSON.stringify({ engine: result.build.engine, differences: result.differences }));
    } finally {
      await server.close();
    }
  }
}
