import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
const state = (id) => JSON.parse(native.liveAuxiliaryState(id));
const entries = async (id) => {
  const value = JSON.parse(await native.liveNetworkLog(id));
  return Array.isArray(value) ? value : value.entries;
};
function publish(alias, url, fakeOwner) {
  native.liveNetworkLogPublishIntercepted(
    alias,
    JSON.stringify({
      url,
      method: "GET",
      status: 200,
      statusText: "OK",
      ok: true,
      headers: [["content-length", "5"]],
      context: { resourceType: "fetch", frame: { path: [], pageId: fakeOwner } },
    }),
    Buffer.from("owned"),
  );
}
async function fixture(run) {
  let arrived, response;
  const held = new Promise((resolve) => {
    arrived = resolve;
  });
  const server = createServer((_request, res) => {
    response = res;
    arrived();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  let actor;
  try {
    await run({
      base,
      held,
      release: () => response?.end(""),
      remember: (id) => {
        actor = id;
      },
    });
  } finally {
    response?.end("");
    if (actor != null) native.liveActorClose(actor);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("intercepted responses use actual root and popup aliases and reject retired root actor tokens", async () => {
  await fixture(async ({ base, remember }) => {
    const root = await native.liveOpen("<body>root</body>", base, "[]", "");
    remember(root);
    const rootTarget = state(root).root;
    publish(root, `${base}/root-result`, "forged");
    assert.equal(
      (await entries(root)).find((entry) => entry.url === `${base}/root-result`).context.frame
        .pageId,
      rootTarget,
    );
    await native.liveEvalEngineExpression(
      root,
      "(() => {window.child=window.open('about:blank','actual-child');return true;})()",
    );
    const childTarget = state(root).pages.find((page) => page.target !== rootTarget).target;
    const child = native.liveAuxiliaryAdopt(root, childTarget);
    native.liveClose(root);
    assert.throws(() => publish(root, `${base}/retired-root`, childTarget), /closed|no such live/);
    publish(child, `${base}/child-result`, rootTarget);
    const rows = await entries(child);
    assert.equal(
      rows.find((entry) => entry.url === `${base}/child-result`).context.frame.pageId,
      childTarget,
    );
    assert.equal(
      rows.some((entry) => entry.url === `${base}/retired-root`),
      false,
    );
  });
});

test("initial intercepted responses use the registered opening root before its alias is published", async () => {
  await fixture(async ({ base, held, release, remember }) => {
    const reserved = native.liveNetworkLogCreate();
    remember(reserved);
    const opened = native.liveOpen(
      '<script src="/held.js"></script>',
      base,
      "[]",
      "",
      undefined,
      JSON.stringify({ hostNetworkLog: reserved }),
    );
    opened.catch(() => {});
    await held;
    const initial = state(reserved);
    assert.equal(initial.opening, true);
    assert.equal(typeof initial.root, "string");
    publish(reserved, `${base}/opening-result`, "forged-child");
    assert.equal(
      (await entries(reserved)).find((entry) => entry.url === `${base}/opening-result`).context
        .frame.pageId,
      initial.root,
    );
    release();
    assert.equal(await opened, reserved);
  });
});
