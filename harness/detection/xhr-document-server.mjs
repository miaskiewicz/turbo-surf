import { createServer } from "node:http";
// The responses of harness/detection/xhr-document-probe.js (path -> content type, body).
export const XHR_DOCUMENT_ROUTES = {
  "/xml": ["application/xml", '<?xml version="1.0"?><a xmlns:p="urn:p"><b p:c="1">t</b></a>'],
  "/text-xml": ["text/xml", "<a><![CDATA[x<y]]></a>"],
  "/svg": ["image/svg+xml", '<svg xmlns="http://www.w3.org/2000/svg"><g/></svg>'],
  "/xhtml": [
    "application/xhtml+xml",
    '<html xmlns="http://www.w3.org/1999/xhtml"><body>x</body></html>',
  ],
  "/bad": ["application/xml", "<a><b></a>"],
  "/html": ["text/html", "<!doctype html><title>t</title><p>x"],
  "/plain": ["text/plain", "<a>plain</a>"],
};
export async function startXhrDocumentServer() {
  const server = createServer((request, response) => {
    const route = XHR_DOCUMENT_ROUTES[request.url];
    response.setHeader("Connection", "close");
    response.setHeader("Content-Type", route ? route[0] : "text/html");
    response.end(route ? route[1] : "<!doctype html><body>XHR document control");
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return {
    url: `http://127.0.0.1:${server.address().port}/`,
    close: async () => {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
