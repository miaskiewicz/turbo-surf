(async () => {
  const out = {};
  const safe = (promise) => promise.catch((error) => ({ error: error?.name || String(error) }));
  const inspect = async (origin, id) => (await fetch(origin + "/state?id=" + id)).json();
  const seen = async (origin, id) => {
    for (let attempt = 0; attempt < 12; attempt++) {
      if ((await inspect(origin, id)).seen) return true;
      await new Promise((done) => setTimeout(done, 20));
    }
    return false;
  };
  const streamed = async (origin, id) => {
    let controller;
    const body = new ReadableStream({
      start(value) {
        controller = value;
        value.enqueue(new Uint8Array([65]));
      },
    });
    const request = safe(
      fetch(origin + "/upload?id=" + id, { method: "POST", body, duplex: "half" }).then(
        (response) => response.json(),
      ),
    );
    const beforeClose = await seen(origin, id);
    controller.enqueue(new Uint8Array([66]));
    controller.close();
    return { beforeClose, result: await request };
  };
  out.sameOrigin = await streamed(location.origin, "same");
  out.crossOrigin = await streamed(globalThis.__uploadPeer, "cross");
  const closed = () =>
    new ReadableStream({
      start(c) {
        c.enqueue(new Uint8Array([65]));
        c.close();
      },
    });
  out.http1 = await safe(
    fetch(globalThis.__uploadHttp1 + "/upload?id=h1", {
      method: "POST",
      body: closed(),
      duplex: "half",
    }).then((response) => response.json()),
  );
  for (const code of [303, 307]) {
    out["redirect" + code] = await safe(
      fetch("/redirect" + code + "?id=r" + code, {
        method: "POST",
        body: closed(),
        duplex: "half",
      }).then((response) => response.json()),
    );
  }
  let cancelReason = "";
  const aborter = new AbortController();
  const abortBody = new ReadableStream({
    start(value) {
      value.enqueue(new Uint8Array([65]));
    },
    cancel(reason) {
      cancelReason = String(reason);
    },
  });
  const aborted = safe(
    fetch("/upload?id=abort", {
      method: "POST",
      body: abortBody,
      duplex: "half",
      signal: aborter.signal,
    }),
  );
  const beforeAbort = await seen(location.origin, "abort");
  aborter.abort("authored-stop");
  const abortResult = await aborted;
  for (let attempt = 0; attempt < 12; attempt++) {
    if ((await inspect(location.origin, "abort")).aborted) break;
    await new Promise((done) => setTimeout(done, 20));
  }
  out.abort = {
    beforeAbort,
    result: abortResult,
    cancelReason,
    serverAborted: (await inspect(location.origin, "abort")).aborted,
  };
  const broken = new ReadableStream({
    start(c) {
      c.error(new Error("authored-source"));
    },
  });
  out.sourceError = await safe(
    fetch("/upload?id=error", { method: "POST", body: broken, duplex: "half" }),
  );
  let pulls = 0;
  const heldBody = new ReadableStream({
    pull(c) {
      pulls++;
      c.enqueue(new Uint8Array(64 * 1024));
      if (pulls === 64) c.close();
    },
  });
  const held = safe(
    fetch("/upload-hold?id=held", { method: "POST", body: heldBody, duplex: "half" }).then(
      (response) => response.json(),
    ),
  );
  await new Promise((done) => setTimeout(done, 150));
  const bounded = pulls < 32;
  await fetch("/release?id=held");
  out.backpressure = { bounded, result: await held };
  let earlyController,
    earlySettled = false;
  const earlyBody = new ReadableStream({
    start(c) {
      earlyController = c;
      c.enqueue(new Uint8Array([65]));
    },
  });
  const early = safe(
    fetch("/upload-early?id=early", { method: "POST", body: earlyBody, duplex: "half" }).then(
      (response) => {
        earlySettled = true;
        return response.json();
      },
    ),
  );
  await new Promise((done) => setTimeout(done, 150));
  const beforeClose = earlySettled;
  earlyController.close();
  out.earlyResponse = { beforeClose, result: await early };
  return out;
})();
