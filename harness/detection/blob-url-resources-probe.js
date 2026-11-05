(async () => {
  // blob: URLs as script, module, stylesheet, image and iframe sources (turbo-surf#146):
  // the creating storage key's store, revocation timing, MIME checks, the blob
  // document's origin and Content-Security-Policy source matching.
  const out = {};
  const blob = (parts, type) => URL.createObjectURL(new Blob(parts, { type }));
  const settle = (element) =>
    new Promise((resolve) => {
      element.onload = () => resolve("load");
      element.onerror = () => resolve("error");
    });
  const script = async (url, setup = () => {}, doc = document) => {
    const element = doc.createElement("script");
    setup(element);
    element.src = url;
    const done = settle(element);
    doc.head.append(element);
    const result = await done;
    element.remove();
    return result;
  };
  const png = Uint8Array.from(
    atob(
      "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAIAAAASFvFNAAAAF0lEQVR4nGP4z8DAAMGMTMwsrGzsHJwALYwDK9eJPywAAAAASUVORK5CYII=",
    ),
    (character) => character.charCodeAt(0),
  );
  const image = async (url, doc = document) => {
    const element = doc.createElement("img");
    const done = settle(element);
    element.src = url;
    const result = await done;
    return [result, element.naturalWidth, element.naturalHeight];
  };

  // Classic scripts.
  globalThis.blobRuns = [];
  out.script = [
    await script(
      blob(
        ["blobRuns.push(['classic', document.currentScript.src.startsWith('blob:')])"],
        "text/javascript",
      ),
    ),
    blobRuns.splice(0),
  ];
  out.scriptPlainMime = [
    await script(blob(["blobRuns.push('plain')"], "text/plain")),
    blobRuns.splice(0),
  ];
  out.scriptUntyped = [await script(blob(["blobRuns.push('untyped')"], "")), blobRuns.splice(0)];
  out.scriptImageMime = [
    await script(blob(["blobRuns.push('image')"], "image/png")),
    blobRuns.splice(0),
  ];
  out.scriptCrossorigin = [
    await script(blob(["blobRuns.push('cors')"], "text/javascript"), (element) => {
      element.crossOrigin = "anonymous";
    }),
    blobRuns.splice(0),
  ];
  const revoked = blob(["blobRuns.push('revoked')"], "text/javascript");
  URL.revokeObjectURL(revoked);
  out.scriptRevoked = [await script(revoked), blobRuns.splice(0)];
  out.scriptRevokedAfterSrc = await (async () => {
    const url = blob(["blobRuns.push('after src')"], "text/javascript");
    const element = document.createElement("script");
    element.src = url;
    URL.revokeObjectURL(url);
    const done = settle(element);
    document.head.append(element);
    return [await done, blobRuns.splice(0)];
  })();
  out.scriptRevokedAfterInsert = await (async () => {
    const url = blob(["blobRuns.push('after insert')"], "text/javascript");
    const element = document.createElement("script");
    element.src = url;
    const done = settle(element);
    document.head.append(element);
    URL.revokeObjectURL(url);
    return [await done, blobRuns.splice(0)];
  })();
  out.scriptFragment = [
    await script(`${blob(["blobRuns.push('fragment')"], "text/javascript")}#x`),
    blobRuns.splice(0),
  ];
  out.scriptForeign = [
    await script(
      blob(["blobRuns.push('foreign')"], "text/javascript").replace(
        location.origin,
        "http://example.invalid",
      ),
    ),
    blobRuns.splice(0),
  ];
  out.scriptErrorDetail = await (async () => {
    const seen = [];
    const listener = (event) => {
      event.preventDefault();
      seen.push([event.message, event.lineno, event.colno]);
    };
    addEventListener("error", listener);
    await script(blob(["\n  throw new Error('from blob')"], "text/javascript"));
    removeEventListener("error", listener);
    return seen.map(([message, line, column]) => [message, line, column]);
  })();

  // Module scripts and imports.
  const dependency = blob(["export const value = 'dependency';"], "text/javascript");
  out.module = [
    await script(
      blob(
        [
          `import { value } from ${JSON.stringify(dependency)}; blobRuns.push(['module', value, import.meta.url.startsWith('blob:')]);`,
        ],
        "text/javascript",
      ),
      (element) => {
        element.type = "module";
      },
    ),
    blobRuns.splice(0),
  ];
  out.moduleDynamic = await import(dependency).then(
    (module) => module.value,
    (value) => [value?.name, String(value?.message).split(dependency).join("<url>")],
  );
  out.modulePlainMime = [
    await script(blob(["blobRuns.push('module plain')"], "text/plain"), (element) => {
      element.type = "module";
    }),
    blobRuns.splice(0),
  ];

  // Stylesheets.
  const probe = document.createElement("div");
  probe.id = "blob-style-probe";
  document.body.append(probe);
  const stylesheet = async (url) => {
    const element = document.createElement("link");
    element.rel = "stylesheet";
    element.href = url;
    const done = settle(element);
    document.head.append(element);
    const result = await done;
    const rules = (() => {
      try {
        return element.sheet ? element.sheet.cssRules.length : null;
      } catch (value) {
        return value.name;
      }
    })();
    const color = getComputedStyle(probe).color;
    element.remove();
    return [result, rules, color];
  };
  out.style = await stylesheet(blob(["#blob-style-probe { color: rgb(1, 2, 3) }"], "text/css"));
  out.stylePlainMime = await stylesheet(
    blob(["#blob-style-probe { color: rgb(4, 5, 6) }"], "text/plain"),
  );
  const revokedStyle = blob(["#blob-style-probe { color: rgb(7, 8, 9) }"], "text/css");
  URL.revokeObjectURL(revokedStyle);
  out.styleRevoked = await stylesheet(revokedStyle);
  out.styleImport = await (async () => {
    const imported = blob(["#blob-style-probe { color: rgb(10, 11, 12) }"], "text/css");
    return stylesheet(blob([`@import url(${JSON.stringify(imported)});`], "text/css"));
  })();

  // Images.
  out.image = await image(blob([png], "image/png"));
  out.imageUntyped = await image(blob([png], ""));
  out.imageGarbage = await image(blob(["not an image"], "image/png"));
  const revokedImage = blob([png], "image/png");
  URL.revokeObjectURL(revokedImage);
  out.imageRevoked = await image(revokedImage);

  // Iframes: a blob document inherits the creator's origin.
  const frame = async (url) => {
    const element = document.createElement("iframe");
    element.src = url;
    const done = settle(element);
    document.body.append(element);
    const result = await done;
    const view = (() => {
      try {
        const doc = element.contentDocument;
        return [
          element.contentWindow.location.href === url,
          element.contentWindow.origin === location.origin,
          doc?.contentType ?? null,
          doc?.body?.textContent ?? null,
          doc?.title ?? null,
        ];
      } catch (value) {
        return [value.name];
      }
    })();
    element.remove();
    return [result, view];
  };
  out.iframe = await frame(
    blob(["<!doctype html><title>blob frame</title><body>framed"], "text/html"),
  );
  out.iframeText = await frame(blob(["plain <b>text</b>"], "text/plain"));
  const revokedFrame = blob(["<!doctype html><body>revoked frame"], "text/html");
  URL.revokeObjectURL(revokedFrame);
  out.iframeRevoked = await frame(revokedFrame);

  // Content-Security-Policy: blob: matches only an explicit blob: scheme source.
  const policyFrame = async (policy) => {
    const element = document.createElement("iframe");
    element.srcdoc = `<!doctype html><meta http-equiv="Content-Security-Policy" content="${policy}"><body>`;
    const loaded = settle(element);
    document.body.append(element);
    await loaded;
    const doc = element.contentDocument,
      win = element.contentWindow;
    const violations = [];
    doc.addEventListener("securitypolicyviolation", (event) =>
      violations.push([event.effectiveDirective, event.blockedURI]),
    );
    win.blobRuns = [];
    const result = [
      await script(blob(["blobRuns.push('csp script')"], "text/javascript"), () => {}, doc),
      [...win.blobRuns],
      (await image(blob([png], "image/png"), doc))[0],
    ];
    await new Promise((resolve) => setTimeout(resolve, 50));
    element.remove();
    return [...result, violations.sort()];
  };
  out.cspSelf = await policyFrame("script-src 'self'; img-src 'self'");
  out.cspBlob = await policyFrame("script-src blob:; img-src blob:");
  out.cspStar = await policyFrame("script-src *; img-src *");
  probe.remove();
  return JSON.stringify(out);
})();
