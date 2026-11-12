import { createJSHandle } from "./js-handles.mjs";
export function consoleMessage(page, entry) {
  const owner =
    entry.contextId === page._requestContextId ? page : page._requestFrames?.get(entry.contextId);
  const handles = entry.args.map((descriptor) => createJSHandle(owner ?? page, descriptor));
  return {
    type: () => entry.type,
    text: () => entry.text,
    args: () => handles.slice(),
    location: () => ({ ...entry.location }),
    page: () => page,
  };
}
