// State stays in the native ledger. This mirror carries only owned entry identities.
const identity = (value) => typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value);

const localIdentity = (value) => Number.isInteger(value) && value >= 0 && value <= 0xffffffff;

function validateMetadataOnly(entry) {
  if (Object.hasOwn(entry, "state") || Object.hasOwn(entry, "serialized"))
    throw new Error("Native history metadata must not contain guest state");
}

function validateEntry(entry, rows, locals) {
  if (
    !entry ||
    !identity(entry.row) ||
    typeof entry.url !== "string" ||
    !localIdentity(entry.local) ||
    (entry.group !== null && !identity(entry.group)) ||
    (entry.group === null && entry.local !== 0) ||
    rows.has(entry.row)
  )
    throw new Error("Native history entry identity is invalid");
  validateMetadataOnly(entry);
  const local = entry.local === 0 ? null : `${entry.group}:${entry.local}`;
  if (local && locals.has(local)) throw new Error("Native history local identity is duplicated");
  rows.add(entry.row);
  if (local) locals.add(local);
}

export function validateHistorySnapshot(snapshot) {
  if (
    !snapshot ||
    snapshot.closed !== false ||
    !identity(snapshot.revision) ||
    !identity(snapshot.document) ||
    !identity(snapshot.group) ||
    !Array.isArray(snapshot.entries) ||
    !Number.isInteger(snapshot.index) ||
    snapshot.index < 0 ||
    snapshot.index >= snapshot.entries.length
  )
    throw new Error("Native target history snapshot is unavailable or closed");
  const rows = new Set();
  const locals = new Set();
  for (const entry of snapshot.entries) validateEntry(entry, rows, locals);
  const current = snapshot.entries[snapshot.index];
  if (current.group !== snapshot.group || current.local === 0)
    throw new Error("Native current history entry has no owned document group");
  return snapshot;
}

function descriptor(entry, snapshot, session) {
  if (entry.group === null || entry.local === 0) return entry.url;
  return {
    row: entry.row,
    id: entry.local,
    url: entry.url,
    group: `${session}:${entry.group}`,
    document: entry.group === snapshot.group ? `${session}:${snapshot.document}` : null,
  };
}

export function hasNativeHistory(page) {
  if (page._session == null || !page._auxiliaryActor) return false;
  if (
    page._actorHub?.page?.(page._targetId) !== page ||
    page._actorHub?.canReadTarget(page._targetId) !== true
  )
    throw new Error("Native history target ownership is unavailable");
  return true;
}

export function syncNativeHistory(page, native) {
  if (!hasNativeHistory(page)) return false;
  if (typeof native.liveTargetHistoryState !== "function")
    throw new Error("Native target history capability is unavailable");
  const snapshot = validateHistorySnapshot(
    JSON.parse(native.liveTargetHistoryState(page._session)),
  );
  const previous = page._nativeHistory;
  const session = String(page._session);
  if (previous?.session === session && BigInt(snapshot.revision) < BigInt(previous.revision))
    throw new Error("Native history revision moved backwards");
  const entries = snapshot.entries.map((entry) => descriptor(entry, snapshot, session));
  const current = entries[snapshot.index];
  page._history = entries.slice(0, snapshot.index);
  page._fwd = entries.slice(snapshot.index + 1).reverse();
  page._historyCurrent = current;
  page._url = current.url;
  page._nativeHistory = {
    session,
    revision: snapshot.revision,
    document: snapshot.document,
    group: snapshot.group,
    row: current.row,
    local: current.id,
  };
  return true;
}

export function retiredNativeHistoryTarget(page) {
  const actor = page._actorHub;
  return (
    page._closed ||
    page._session == null ||
    actor?.nativeClosed === true ||
    actor?.active === false ||
    actor?.retired?.has(page._targetId) === true ||
    actor?.info?.get(page._targetId)?.closed === true
  );
}

const eventFields = ["document", "group", "revision", "row", "local", "url"];

function validateHistoryEvent(page, event) {
  if (
    String(page._targetId) !== event.target ||
    !identity(event.document) ||
    !identity(event.group) ||
    !identity(event.revision) ||
    !identity(event.row) ||
    !localIdentity(event.local) ||
    event.local === 0 ||
    typeof event.url !== "string"
  )
    throw new Error("Native history notification has no owned entry identity");
}

export function notifyNativeHistory(page, native, event) {
  if (retiredNativeHistoryTarget(page)) return false;
  validateHistoryEvent(page, event);
  const session = String(page._session);
  const previous = page._nativeHistoryEvent;
  if (previous?.session === session && BigInt(event.revision) <= BigInt(previous.revision)) {
    if (eventFields.every((field) => previous[field] === event[field])) return false;
    throw new Error("Native history notifications are out of order");
  }
  if (!syncNativeHistory(page, native))
    throw new Error("Native history notification has no target");
  page._nativeHistoryEvent = {
    session,
    ...Object.fromEntries(eventFields.map((field) => [field, event[field]])),
  };
  emitNativeFrameNavigation(page, event.url);
  return true;
}

export function emitNativeFrameNavigation(page, url) {
  if (typeof url !== "string") throw new Error("Native navigation URL is unavailable");
  const current = page._url;
  try {
    // Deliver ordered native history/document events without rolling the ledger
    // mirror backwards when a newer native snapshot already exists.
    page._url = url;
    page._emit("framenavigated", page);
  } finally {
    page._url = current;
  }
}
