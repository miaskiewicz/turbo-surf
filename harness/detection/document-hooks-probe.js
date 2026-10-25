(() => {
  const pad = (value) => String(value).padStart(2, "0");
  const local = (date) =>
    `${pad(date.getMonth() + 1)}/${pad(date.getDate())}/${String(date.getFullYear()).padStart(4, "0")} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  const parsed = (text) => {
    const match = /^(\d\d)\/(\d\d)\/(\d{4}) (\d\d):(\d\d):(\d\d)$/.exec(text);
    return match
      ? new Date(+match[3], match[1] - 1, +match[2], +match[4], +match[5], +match[6]).getTime()
      : NaN;
  };
  // The authored data-lm attribute restates the response header; both sides are
  // formatted in this page's own time zone, so the verdict is zone independent.
  const lastModified = (doc) => {
    const text = doc.lastModified;
    const header = doc.documentElement && doc.documentElement.getAttribute("data-lm");
    if (header !== null && header !== undefined && text === local(new Date(Date.parse(header))))
      return "header";
    if (Math.abs(parsed(text) - Date.now()) < 120000) return "now";
    return `other:${text}`;
  };
  const attempt = (action) => {
    try {
      return { value: action() };
    } catch (error) {
      return { error: `${error.name}: ${error.message}` };
    }
  };
  // Cross-origin access messages name the control's ephemeral port; keep the verdict.
  const reach = (action) => {
    try {
      return { value: action() };
    } catch (error) {
      return { error: error.name };
    }
  };
  const facts = (win, target) => {
    const doc = win.document,
      out = {
        lastModified: lastModified(doc),
        domain: doc.domain,
        originAgentCluster: win.originAgentCluster,
        fullscreenEnabled: doc.fullscreenEnabled,
        webkitFullscreenEnabled: doc.webkitFullscreenEnabled,
        pictureInPictureEnabled: doc.pictureInPictureEnabled,
      };
    if (target !== null) {
      out.set = attempt(() => {
        doc.domain = target;
        return "ok";
      });
      out.after = doc.domain;
    }
    return out;
  };
  const fn = (value) =>
    typeof value === "function"
      ? { name: value.name, length: value.length, source: Function.prototype.toString.call(value) }
      : value === undefined
        ? "undefined"
        : typeof value;
  const describe = (object, name) => {
    const descriptor = Object.getOwnPropertyDescriptor(object, name);
    if (!descriptor) return null;
    return {
      get: fn(descriptor.get),
      set: fn(descriptor.set),
      value: "value" in descriptor ? typeof descriptor.value : "absent",
      writable: "writable" in descriptor ? descriptor.writable : "absent",
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
    };
  };
  const shape = () => {
    const rows = {};
    const detachedFrame = document.createElement("iframe");
    document.body.appendChild(detachedFrame);
    const detached = detachedFrame.contentDocument;
    detachedFrame.remove();
    const inert = {
      createHTMLDocument: document.implementation.createHTMLDocument(""),
      newDocument: new Document(),
      domParser: new DOMParser().parseFromString("<p>x", "text/html"),
      detached,
    };
    for (const name of ["fullscreenEnabled", "webkitFullscreenEnabled", "pictureInPictureEnabled"]) {
      const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, name) || {};
      const row = {
        descriptor: describe(Document.prototype, name),
        own: Object.hasOwn(document, name),
        foreignGet: attempt(() => descriptor.get.call({})),
        nullGet: attempt(() => descriptor.get.call(null)),
        elementGet: attempt(() => descriptor.get.call(document.body)),
        foreignSet: descriptor.set ? attempt(() => descriptor.set.call({}, false)) : "absent",
        selfSet: descriptor.set
          ? attempt(() => {
              descriptor.set.call(document, !document[name]);
              return [document[name], Object.hasOwn(document, name)];
            })
          : "absent",
        sloppyAssign: attempt(() => {
          document[name] = "assigned";
          return [document[name], Object.hasOwn(document, name)];
        }),
        strictAssign: attempt(() =>
          (function () {
            "use strict";
            document[name] = "assigned";
            return [document[name], Object.hasOwn(document, name)];
          })(),
        ),
      };
      for (const [label, doc] of Object.entries(inert)) row[label] = attempt(() => doc[name]);
      rows[name] = row;
    }
    rows.originAgentCluster = {
      window: describe(window, "originAgentCluster"),
      prototype: describe(Window.prototype, "originAgentCluster"),
      foreignGet: attempt(() =>
        Object.getOwnPropertyDescriptor(window, "originAgentCluster").get.call({}),
      ),
      sloppyAssign: attempt(() => {
        window.originAgentCluster = "assigned";
        return [window.originAgentCluster, typeof Object.getOwnPropertyDescriptor(window, "originAgentCluster").get];
      }),
    };
    return rows;
  };
  const root = document.documentElement;
  const result = {};
  if (root.hasAttribute("data-shape")) result.shape = shape();
  const frames = {};
  for (const frame of document.querySelectorAll("iframe")) {
    frames[frame.getAttribute("name")] = reach(() => facts(frame.contentWindow, frame.getAttribute("data-domain")));
  }
  result.frames = frames;
  result.messages = (window.__messages || [])
    .slice()
    .sort((left, right) => (left.name < right.name ? -1 : left.name > right.name ? 1 : 0));
  result.top = facts(window, root.getAttribute("data-domain"));
  return result;
})();
