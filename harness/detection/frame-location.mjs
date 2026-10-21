// Measure genuine Chrome Location restrictions, frame ordering and retirement.
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";
const here = fileURLToPath(new URL(".", import.meta.url));
const out = resolve(process.argv[2] || join(here, "artifacts", `frame-location-${Date.now()}`));
mkdirSync(out, { recursive: true, mode: 0o700 });
const { chromium } = createRequire(join(here, "../../scripts/browser-sidecar/package.json"))(
  "patchright",
);
const serve = () =>
  createServer((q, r) => {
    r.setHeader("Content-Type", "text/html");
    r.end(
      q.url.startsWith("/parent")
        ? "<body></body>"
        : `<body><script>parent.postMessage({path:location.pathname},"*")</script></body>`,
    );
  });
const parent = serve(),
  other = serve();
await Promise.all([parent, other].map((s) => new Promise((r) => s.listen(0, "127.0.0.1", r))));
let browser;
try {
  browser = await chromium.launch({ channel: "chrome", headless: true });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${parent.address().port}/parent/dir/page`);
  const result = await page.evaluate(
    async ({ otherBase }) => {
      const capture = (fn) => {
        try {
          return { value: fn() };
        } catch (e) {
          return { error: e.name, branded: e instanceof DOMException, code: e.code };
        }
      };
      const descriptor = (obj, key) => {
        const d = Object.getOwnPropertyDescriptor(obj, key);
        return d
          ? {
              enumerable: d.enumerable,
              configurable: d.configurable,
              writable: d.writable,
              get: d.get?.name,
              set: d.set?.name,
              value: typeof d.value,
            }
          : null;
      };
      const nav = (frame, fn) =>
        new Promise((resolve, reject) => {
          frame.addEventListener("load", resolve, { once: true });
          try {
            fn();
          } catch (e) {
            reject(e);
          }
        });
      const frame = document.createElement("iframe");
      document.body.append(frame);
      const oldDoc = frame.contentDocument,
        win = frame.contentWindow,
        oldLocation = win.location;
      const sameOriginLocation = {
        tag: Object.prototype.toString.call(oldLocation),
        keys: Reflect.ownKeys(oldLocation).map(String),
        prototypeTag: descriptor(Location.prototype, Symbol.toStringTag),
      };
      const retired = [];
      win.setTimeout(() => retired.push("old"), 150);
      await nav(frame, () => (frame.src = otherBase + "/first"));
      const loc = win.location;
      const operations = {
        locationIdentity: () => loc === win.location,
        oldLocationIdentity: () => oldLocation === loc,
        prototype: () => Object.getPrototypeOf(loc) === null,
        keys: () => Reflect.ownKeys(loc).map(String),
        enumerableKeys: () => Object.keys(loc),
        href: () => loc.href,
        origin: () => loc.origin,
        assign: () => typeof loc.assign,
        reload: () => typeof loc.reload,
        toString: () => String(loc),
        thenValue: () => loc.then,
        tag: () => loc[Symbol.toStringTag],
        replace: () => typeof loc.replace,
        replaceIdentity: () => loc.replace === loc.replace,
        replaceRealm: () => loc.replace.constructor === Function,
        replaceDescriptor: () => descriptor(loc, "replace"),
        hrefDescriptor: () => descriptor(loc, "href"),
        windowLocationDescriptor: () => descriptor(win, "location"),
        windowEnumerableKeys: () => Object.keys(win),
        windowDescriptors: () =>
          Reflect.ownKeys(win).map((key) => [String(key), descriptor(win, key)]),
        hasHref: () => "href" in loc,
        deleteHref: () => delete loc.href,
        setPrototype: () => Object.setPrototypeOf(loc, {}) === loc,
        defineHref: () => Object.defineProperty(loc, "href", { value: "x" }),
        illegalReplaceReceiver: () => loc.replace.call({}, "/ignored"),
        missingReplaceArgument: () => loc.replace(),
        oldDocumentView: () => oldDoc.defaultView === win,
        oldDocumentViewNull: () => oldDoc.defaultView === null,
      };
      const output = {
        sameOriginLocation,
        operations: Object.fromEntries(
          Object.entries(operations).map(([k, fn]) => [k, capture(fn)]),
        ),
        navigation: {},
      };
      await nav(frame, () => (loc.href = "../via-href"));
      output.navigation.href = frame.contentDocument.URL;
      output.navigation.retainedLocation = capture(() => loc.href);
      await nav(frame, () => (frame.src = otherBase + "/again"));
      await nav(frame, () => frame.contentWindow.location.replace("../via-replace"));
      output.navigation.replace = frame.contentDocument.URL;
      await nav(frame, () => (frame.src = otherBase + "/again"));
      await nav(frame, () => (frame.contentWindow.location = "../via-window"));
      output.navigation.window = frame.contentDocument.URL;
      await nav(frame, () => frame.removeAttribute("src"));
      output.navigation.removeSrc = {
        url: frame.contentDocument.URL,
        sameWindow: frame.contentWindow === win,
        oldDocView: oldDoc.defaultView === win,
      };
      frame.remove();
      output.navigation.removedView = oldDoc.defaultView === win;
      await new Promise((r) => setTimeout(r, 220));
      output.retiredCallbacks = retired;
      const a = document.createElement("iframe"),
        b = document.createElement("iframe");
      document.body.append(a, b);
      const aw = a.contentWindow,
        bw = b.contentWindow;
      output.order = { initial: window[0] === aw && window[1] === bw };
      document.body.insertBefore(b, a);
      output.order.reordered = window[0] === b.contentWindow && window[1] === a.contentWindow;
      output.order.reorderedMatches = [
        window[0] === a.contentWindow,
        window[0] === b.contentWindow,
        window[1] === a.contentWindow,
        window[1] === b.contentWindow,
      ];
      await new Promise((r) => setTimeout(r, 20));
      output.order.reorderedAfterTask =
        window[0] === b.contentWindow && window[1] === a.contentWindow;
      output.order.movedContextReplaced = b.contentWindow !== bw;
      output.order.unmovedContextRetained = a.contentWindow === aw;
      const host = document.createElement("div");
      document.body.append(host);
      const shadow = host.attachShadow({ mode: "closed" });
      const c = document.createElement("iframe");
      shadow.append(c);
      output.order.shadow = { length: window.length, found: window[2] === c.contentWindow };
      return output;
    },
    {
      parentBase: `http://127.0.0.1:${parent.address().port}`,
      otherBase: `http://127.0.0.1:${other.address().port}`,
    },
  );
  writeFileSync(
    join(out, "chrome-frame-location.json"),
    JSON.stringify({ version: await browser.version(), ...result }, null, 2),
    { mode: 0o600 },
  );
  console.log(JSON.stringify({ out, ...result }));
} finally {
  await browser?.close();
  parent.close();
  other.close();
}
