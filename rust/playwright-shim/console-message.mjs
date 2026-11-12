import { createJSHandle } from "./js-handles.mjs";
export function consoleMessage(page, entry) {
  const session = page._session ?? page._openingNetworkId;
  const resolveOwner = async () => {
    await page._openingSession?.promise;
    if (page._closed || page._session !== session)
      throw new Error("Console argument execution context was destroyed");
    const find = () =>
      entry.contextId === page._requestContextId ? page : page._requestFrames?.get(entry.contextId);
    let owner = find();
    if (!owner) {
      await page._refreshRequestFrames();
      page._syncFrames();
      owner = find();
    }
    if (!owner) throw new Error("Console argument execution context was destroyed");
    return owner;
  };
  const handles = entry.args.map((descriptor) => createJSHandle(page, descriptor, resolveOwner));
  return {
    type: () => entry.type,
    text: () => entry.text,
    args: () => handles.slice(),
    location: () => ({ ...entry.location }),
    page: () => page,
  };
}
