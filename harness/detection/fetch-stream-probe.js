(async () => {
  const controller = new AbortController(),
    reason = { authored: "reason" };
  const request = new Request(location.origin + "/binary", { signal: controller.signal }),
    clone = request.clone(),
    empty = new Request(request, { signal: null });
  controller.abort(reason);
  let invalid = false;
  try {
    new Request(location.origin, { signal: { aborted: false } });
  } catch (error) {
    invalid = error instanceof TypeError;
  }
  let syncThrow = false,
    invalidFetch;
  try {
    invalidFetch = fetch(location.origin, { signal: {} });
  } catch {
    syncThrow = true;
  }
  let rejection = false;
  try {
    await invalidFetch;
  } catch (error) {
    rejection = error instanceof TypeError;
  }
  const headers = await fetch("/hold");
  const held = {
    status: headers.status,
    own: Reflect.ownKeys(headers.body).map(String),
    used: headers.bodyUsed,
    locked: headers.body.locked,
  };
  await headers.body.cancel("finished");
  held.canceledUsed = headers.bodyUsed;
  const abort = new AbortController(),
    response = await fetch("/hold", { signal: abort.signal }),
    reader = response.body.getReader(),
    pending = reader.read();
  abort.abort(reason);
  let readReason = false;
  try {
    await pending;
  } catch (error) {
    readReason = error === reason;
  }
  let consumedReason = false;
  reader.releaseLock();
  try {
    await response.arrayBuffer();
  } catch (error) {
    consumedReason = error === reason;
  }
  const before = new AbortController(),
    beforeReason = new Error("Authored abort"),
    fetchPending = fetch("/pending", { signal: before.signal });
  before.abort(beforeReason);
  let beforeSame = false;
  try {
    await fetchPending;
  } catch (error) {
    beforeSame = error === beforeReason;
  }
  const binary = await fetch("/binary"),
    binaryClone = binary.clone();
  const [first, second] = await Promise.all([binary.bytes(), binaryClone.bytes()]);
  const upload = await fetch("/echo", { method: "POST", body: new Uint8Array([0, 255, 128, 65]) });
  const nullResponse = await fetch("/null"),
    nullClone = nullResponse.clone();
  return {
    request: {
      different: request.signal !== controller.signal && clone.signal !== request.signal,
      allAbort: request.signal.aborted && clone.signal.aborted,
      reason: request.signal.reason === reason && clone.signal.reason === reason,
      nullFresh: !empty.signal.aborted,
      invalid,
      syncThrow,
      rejection,
    },
    held,
    abort: { readReason, consumedReason, beforeSame },
    binary: {
      first: Array.from(first),
      second: Array.from(second),
      type: binary.type,
      statusText: binary.statusText,
      used: binary.bodyUsed,
      cloneUsed: binaryClone.bodyUsed,
    },
    upload: await upload.json(),
    nullBody: {
      null: nullResponse.body === null,
      status: nullResponse.status,
      type: Object.prototype.toString.call(nullResponse.body),
      cloneType: Object.prototype.toString.call(nullClone.body),
      cloneText: await nullClone.text(),
    },
  };
})();
