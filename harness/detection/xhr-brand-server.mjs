import { createServer } from "node:http";
export async function startXhrBrandServer() {
  const server = createServer((request, response) => {
    response.setHeader("Connection", "close");
    response.setHeader("Content-Type", request.url === "/xhr" ? "text/plain" : "text/html");
    response.end(request.url === "/xhr" ? "xhr-response" : "<!doctype html><body>XHR control");
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
