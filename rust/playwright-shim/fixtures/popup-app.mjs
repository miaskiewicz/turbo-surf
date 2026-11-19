import { createServer } from "node:http";

const APP = `<!doctype html><title>Opener</title>
<a id="target-blank" href="/document?kind=anchor" target="_blank">Open link</a>
<button id="document">Open document</button>
<button id="named">Open named</button>
<button id="reuse">Reuse named</button>
<button id="close-child">Close child</button>
<button id="cross-origin">Open other origin</button>
<output id="result"></output>
<script>
const result = document.querySelector('#result');
document.querySelector('#document').onclick = () => {
  const child = window.open('about:blank', '_blank');
  result.textContent = JSON.stringify({opened: child !== null,
    hasOpener: child?.opener === window, initialUrl: child?.document.URL});
  if (!child) return;
  child.document.title = 'Pending document';
  child.opener = null;
  fetch('/presign').then(response => response.text()).then(url => {child.location.href = url;});
};
document.querySelector('#named').onclick = () => {
  window.preview = window.open('/document?kind=named', 'receipt');
};
document.querySelector('#reuse').onclick = () => {
  const next = window.open('/document?kind=reused', 'receipt');
  result.textContent = String(next === window.preview);
};
document.querySelector('#close-child').onclick = () => {
  window.preview.close();
  result.textContent = String(window.preview.closed);
};
document.querySelector('#cross-origin').onclick = () => {
  window.preview = window.open('__PEER__/document?kind=cross-origin', '_blank');
};
</script>`;

export async function createPopupFixture() {
  const requests = [];
  let peerOrigin = "";
  const sockets = new Set();
  const server = createServer((request, response) => {
    const url = new URL(request.url, "http://localhost");
    requests.push({ path: url.pathname, search: url.search, cookie: request.headers.cookie ?? "" });
    response.setHeader("Cache-Control", "no-store");
    response.setHeader("Content-Type", "text/html");
    if (url.pathname === "/presign") {
      response.setHeader("Content-Type", "text/plain");
      response.end("/document?kind=presigned");
    } else if (url.pathname === "/file") {
      response.setHeader("Content-Type", "application/pdf");
      response.setHeader("Content-Disposition", 'attachment; filename="receipt.pdf"');
      response.end("%PDF-1.0\nlocal fixture\n%%EOF");
    } else if (url.pathname === "/document") {
      response.end(`<!doctype html><title>Receipt ${url.searchParams.get("kind")}</title>
        <p id="document">Independent document</p><a id="download" href="/file" download="receipt.pdf">Download</a>`);
    } else if (url.pathname === "/blocked") {
      response.setHeader("Content-Security-Policy", "sandbox allow-scripts allow-same-origin");
      response.end(`<!doctype html><button id="open">Try popup</button><output id="result"></output>
        <script>document.querySelector('#open').onclick = () => {
          document.querySelector('#result').textContent = String(window.open('/document', '_blank') === null);
        };</script>`);
    } else {
      response.end(APP.replaceAll("__PEER__", peerOrigin));
    }
  });
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    requests,
    setPeer(origin) {
      peerOrigin = origin;
    },
    async close() {
      for (const socket of sockets) socket.destroy();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
