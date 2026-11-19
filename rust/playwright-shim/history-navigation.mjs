// The driver mirrors native session-history identities, including distinct entries
// with identical URLs. Only entries in the active native document can traverse live.
export const historyUrl = (entry) => (typeof entry === "string" ? entry : entry.url);

function traverse(page, target) {
  for (const [source, destination] of [
    [page._history, page._fwd],
    [page._fwd, page._history],
  ]) {
    const at = source.findLastIndex(
      (entry) => entry.document === target.document && entry.id === target.id,
    );
    if (at < 0) continue;
    while (source.length > at) {
      destination.push(page._historyCurrent);
      page._historyCurrent = source.pop();
    }
    return;
  }
}

export function syncHistory(page, snapshot) {
  const document = `${page._session}:${snapshot.document}`;
  if (page._historyCurrent?.document !== document)
    page._historyCurrent = { ...snapshot.initial, document };
  for (const change of snapshot.changes) {
    const target = { id: change.id, url: change.url, document };
    if (change.kind === "push") {
      page._history.push(page._historyCurrent);
      page._fwd = [];
    } else if (change.kind === "traverse") traverse(page, target);
    page._historyCurrent = target;
    page._url = target.url;
    page._emit("framenavigated", page);
  }
}

export async function traverseLive(page, entry, native) {
  if (
    !entry?.document ||
    entry.document !== page._historyCurrent?.document ||
    page._session == null
  )
    return false;
  const moved = await native.liveEvalEngine(
    page._session,
    `globalThis.__RESULT=__tsNs.traverseHistoryEntry(${entry.id}) ? 'yes' : '';`,
  );
  if (!moved) return false;
  await page._refreshFromSession();
  return true;
}
