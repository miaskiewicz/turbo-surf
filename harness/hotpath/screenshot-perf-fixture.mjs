import { createServer } from "node:http";

export async function withScreenshotImageSite(native, run) {
  const png = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const requests = [];
  const server = createServer((request, response) => {
    if (request.url.startsWith("/image/")) {
      requests.push(request.url);
      response.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" });
      response.end(png);
      return;
    }
    response.writeHead(200, { "content-type": "text/html" });
    response.end(
      "<!doctype html><style>body{margin:0}img{display:block;width:300px}</style>" +
        Array.from({ length: 6 }, (_, index) => `<img src='/image/${index}.png'>`).join(""),
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    await run({ url: `http://127.0.0.1:${server.address().port}`, requests, png });
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}
