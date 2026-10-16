// Background dictionaries do not hold DOMContentLoaded or the document load event.
const __dictionaryApply = Reflect.apply;
const __dictionaryThen = Promise.prototype.then;
const __dictionaryDefine = Object.defineProperty;
const __dictionarySpecies = { __proto__: null, [Symbol.species]: Promise };
function __dictionaryShield(promise) {
  __dictionaryDefine(promise, "constructor", { __proto__: null, value: __dictionarySpecies });
  return promise;
}
const __dictionaryLinks = new WeakMap();
const __dictionaryGet = WeakMap.prototype.get,
  __dictionarySet = WeakMap.prototype.set,
  __dictionaryDelete = WeakMap.prototype.delete;
const __dictionaryRequests = new Map();
const __dictionaryRequestGet = Map.prototype.get,
  __dictionaryRequestSet = Map.prototype.set,
  __dictionaryRequestDelete = Map.prototype.delete;
const __dictionaryEach = Map.prototype.forEach;
const __dictionaryActive = new Map();
const __dictionaryRangeError = RangeError;
const __dictionaryReport = __print;
let __dictionaryTaskHead = null,
  __dictionaryTaskTail = null,
  __dictionaryTaskCount = 0;
function __dictionaryQueueTask(run) {
  if (__dictionaryTaskCount >= 4096)
    throw new __dictionaryRangeError("Background resource task limit exceeded");
  const task = { __proto__: null, run, next: null };
  if (__dictionaryTaskTail) __dictionaryTaskTail.next = task;
  else __dictionaryTaskHead = task;
  __dictionaryTaskTail = task;
  __dictionaryTaskCount++;
}
function __dictionaryRetireTasks() {
  __dictionaryTaskHead = __dictionaryTaskTail = null;
  __dictionaryTaskCount = 0;
}
ops.op_set_host_capabilities({
  runNextBackgroundResourceTask: () => {
    const task = __dictionaryTaskHead;
    if (!task) return 0;
    __dictionaryTaskHead = task.next;
    if (!__dictionaryTaskHead) __dictionaryTaskTail = null;
    __dictionaryTaskCount--;
    task.run();
    return 1;
  },
});
let __dictionaryPending = 0,
  __dictionaryEpoch = 0;
function __dictionaryRelease(element, generation) {
  if (__dictionaryApply(__dictionaryRequestGet, __dictionaryActive, [element]) === generation)
    __dictionaryApply(__dictionaryRequestDelete, __dictionaryActive, [element]);
  const request = generation.request;
  generation.request = null;
  if (!request || request.settled) return;
  request.owners--;
  if (request.owners === 0 && !request.header) {
    ops.op_fetch_cancel(request.cancel);
    if (__dictionaryApply(__dictionaryRequestGet, __dictionaryRequests, [request.url]) === request)
      __dictionaryApply(__dictionaryRequestDelete, __dictionaryRequests, [request.url]);
  }
}
function __dictionaryFetch(href, base, element, relation, generation) {
  const cancel = ops.op_fetch_prepare(__document);
  let prepared;
  try {
    prepared = ops.op_dictionary_prepare(__document, href, base, relation, cancel);
  } catch {
    ops.op_fetch_cancel(cancel);
    return;
  }
  if (!prepared) {
    ops.op_fetch_cancel(cancel);
    return;
  }
  const fire = (kind) => {
    if (
      element &&
      __dictionaryApply(__dictionaryGet, __dictionaryLinks, [element]) === generation &&
      ops.op_dictionary_active(__document)
    )
      __fireResourceEvent(element, kind);
  };
  if (!prepared[1]) {
    ops.op_fetch_cancel(cancel);
    __dictionaryQueueTask(() => fire("error"));
    return;
  }
  let request = __dictionaryApply(__dictionaryRequestGet, __dictionaryRequests, [prepared[0]]);
  if (request) ops.op_fetch_cancel(cancel);
  else {
    request = {
      __proto__: null,
      url: prepared[0],
      cancel,
      owners: 0,
      header: false,
      settled: false,
      promise: null,
    };
    __dictionaryPending++;
    const finish = (result) => {
      request.settled = true;
      if (
        __dictionaryApply(__dictionaryRequestGet, __dictionaryRequests, [request.url]) === request
      )
        __dictionaryApply(__dictionaryRequestDelete, __dictionaryRequests, [request.url]);
      __dictionaryPending--;
      return { __proto__: null, ok: result?.ok === true, aborted: result?.aborted === true };
    };
    try {
      request.promise = __dictionaryShield(
        __dictionaryApply(
          __dictionaryThen,
          __dictionaryShield(ops.op_fetch(prepared[0], prepared[1])),
          [finish, () => finish(null)],
        ),
      );
    } catch {
      ops.op_fetch_cancel(cancel);
      __dictionaryPending--;
      return;
    }
    __dictionaryApply(__dictionaryRequestSet, __dictionaryRequests, [request.url, request]);
  }
  if (element) {
    request.owners++;
    generation.request = request;
    __dictionaryApply(__dictionaryRequestSet, __dictionaryActive, [element, generation]);
    __dictionaryApply(__dictionaryThen, request.promise, [
      (result) => {
        if (__dictionaryApply(__dictionaryGet, __dictionaryLinks, [element]) === generation)
          __dictionaryApply(__dictionaryRequestDelete, __dictionaryActive, [element]);
        if (!result.aborted) {
          try {
            __dictionaryQueueTask(() => fire(result.ok ? "load" : "error"));
          } catch {
            __dictionaryReport("Background resource task limit exceeded\n", true);
          }
        }
      },
    ]);
  } else request.header = true;
}
function __dictionaryRemoved(root) {
  const links = ops.op_dictionary_links(__document, root);
  for (let index = 0; index < links.length; index++) {
    const element = links[index],
      generation = __dictionaryApply(__dictionaryGet, __dictionaryLinks, [element]);
    if (generation) __dictionaryRelease(element, generation);
    __dictionaryApply(__dictionaryDelete, __dictionaryLinks, [element]);
  }
}

function __discoverDictionaries() {
  const source = ops.op_document_request_source(__document);
  const headers = ops.op_dictionary_headers(__document);
  for (let index = 0; index < headers.length; index++)
    __dictionaryFetch(headers[index], source, null, "compression-dictionary");
  const links = ops.op_dictionary_links(__document, __document);
  const epoch = ++__dictionaryEpoch;
  const base = __dictionaryApply(__resourceBaseURL, __document, []) || source;
  for (let index = 0; index < links.length; index++) {
    const element = links[index];
    const rel = __dictionaryApply(__resourceAttribute, element, ["rel"]) || "";
    const href = __dictionaryApply(__resourceAttribute, element, ["href"]);
    if (href === null) continue;
    const key = rel + "\n" + href + "\n" + base;
    const previous = __dictionaryApply(__dictionaryGet, __dictionaryLinks, [element]);
    if (previous?.key === key) {
      previous.seen = epoch;
      continue;
    }
    const generation = { __proto__: null, key, seen: epoch, request: null };
    __dictionaryApply(__dictionarySet, __dictionaryLinks, [element, generation]);
    __dictionaryFetch(href, base, element, rel, generation);
    if (previous) __dictionaryRelease(element, previous);
  }
  __dictionaryApply(__dictionaryEach, __dictionaryActive, [
    (generation, element) => {
      if (generation.seen !== epoch) {
        __dictionaryRelease(element, generation);
        __dictionaryApply(__dictionaryDelete, __dictionaryLinks, [element]);
      }
    },
  ]);
}
