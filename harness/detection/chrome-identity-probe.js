(async () => {
  const attempt = (callback) => {
    try {
      return callback();
    } catch (error) {
      return { error: error && error.name };
    }
  };
  const thrown = (callback) => {
    try {
      callback();
      return null;
    } catch (error) {
      return `${error.name}: ${error.message}`;
    }
  };
  const path = (dotted) =>
    dotted.split(".").reduce((value, key) => (value == null ? undefined : value[key]), globalThis);
  const names = (value) =>
    value == null ? null : Object.getOwnPropertyNames(value).sort((a, b) => (a < b ? -1 : 1));
  const nav = navigator;
  const uad = nav.userAgentData;
  const hints = [
    "architecture",
    "bitness",
    "formFactors",
    "fullVersionList",
    "model",
    "platformVersion",
    "uaFullVersion",
    "wow64",
  ];
  const high = uad
    ? await uad.getHighEntropyValues(hints).catch((error) => ({ error: error.name }))
    : null;
  const stack = (function probeFrame() {
    return new Error("probe").stack;
  })();
  const date = new Date(Date.UTC(2020, 0, 2, 3, 4, 5));
  const counts = {};
  for (const key of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"])
    counts[key] = attempt(() => Intl.supportedValuesOf(key).length);
  const builtins = [
    "Array.fromAsync",
    "Atomics.pause",
    "AsyncDisposableStack",
    "DisposableStack",
    "Error.captureStackTrace",
    "Error.isError",
    "Float16Array",
    "Intl.DurationFormat",
    "Iterator",
    "Iterator.concat",
    "Iterator.zip",
    "JSON.rawJSON",
    "Map.prototype.getOrInsert",
    "Map.prototype.getOrInsertComputed",
    "Math.f16round",
    "Math.sumPrecise",
    "Promise.try",
    "RegExp.escape",
    "ShadowRealm",
    "SuppressedError",
    "Symbol.dispose",
    "Temporal",
    "Uint8Array.fromBase64",
    "Uint8Array.prototype.toHex",
    "WeakRef",
  ];
  const css = [
    "anchor-name: --a",
    "position-try-fallbacks: flip-block",
    "interpolate-size: allow-keywords",
    "field-sizing: content",
    "text-box: trim-both cap alphabetic",
    "reading-flow: flex-visual",
    "corner-shape: squircle",
    "dynamic-range-limit: standard",
    "width: calc(sibling-index() * 1px)",
    "color: if(style(--x: 1): red; else: blue)",
    "clip-path: shape(from 0 0, line to 1px 1px)",
    "scroll-marker-group: after",
    "view-transition-class: a",
    "text-wrap: pretty",
    "color: contrast-color(red)",
    "font-size: 1rcap",
    "anchor-scope: all",
    "overlay: auto",
    "timeline-scope: --t",
    "content-visibility: auto",
  ];
  return {
    navigator: {
      userAgent: nav.userAgent,
      appVersion: nav.appVersion,
      appName: nav.appName,
      appCodeName: nav.appCodeName,
      product: nav.product,
      productSub: nav.productSub,
      vendor: nav.vendor,
      vendorSub: nav.vendorSub,
      platform: nav.platform,
      language: nav.language,
      languages: Array.from(nav.languages || []),
      hardwareConcurrency: nav.hardwareConcurrency,
      deviceMemory: nav.deviceMemory,
      maxTouchPoints: nav.maxTouchPoints,
      webdriver: nav.webdriver,
      cookieEnabled: nav.cookieEnabled,
      doNotTrack: nav.doNotTrack,
      pdfViewerEnabled: nav.pdfViewerEnabled,
      onLine: nav.onLine,
      plugins: Array.from(nav.plugins || [], (plugin) => plugin.name),
      mimeTypes: Array.from(nav.mimeTypes || [], (type) => type.type),
      connectionType: nav.connection ? nav.connection.effectiveType : null,
      ownKeys: Reflect.ownKeys(nav).map(String),
    },
    userAgentData: uad
      ? {
          brands: uad.brands,
          mobile: uad.mobile,
          platform: uad.platform,
          json: attempt(() => uad.toJSON()),
          high,
        }
      : null,
    screen: {
      width: screen.width,
      height: screen.height,
      availWidth: screen.availWidth,
      availHeight: screen.availHeight,
      colorDepth: screen.colorDepth,
      pixelDepth: screen.pixelDepth,
      devicePixelRatio: globalThis.devicePixelRatio,
    },
    intl: {
      dateTime: attempt(() => {
        const options = new Intl.DateTimeFormat().resolvedOptions();
        return {
          locale: options.locale,
          calendar: options.calendar,
          numberingSystem: options.numberingSystem,
          timeZone: options.timeZone,
        };
      }),
      counts,
      samples: {
        dateTime: attempt(() => date.toLocaleString("en-US", { timeZone: "UTC" })),
        dateLong: attempt(() =>
          date.toLocaleDateString("en-GB", { timeZone: "UTC", dateStyle: "full" }),
        ),
        currency: attempt(() =>
          new Intl.NumberFormat("de-DE", { style: "currency", currency: "EUR" }).format(1234.5),
        ),
        compact: attempt(() =>
          new Intl.NumberFormat("en", { notation: "compact" }).format(1234567),
        ),
        relative: attempt(() => new Intl.RelativeTimeFormat("en").format(-1, "day")),
        list: attempt(() => new Intl.ListFormat("en").format(["a", "b", "c"])),
        region: attempt(() => new Intl.DisplayNames(["en"], { type: "region" }).of("MM")),
        plural: attempt(() => new Intl.PluralRules("ar").resolvedOptions().pluralCategories),
      },
    },
    engine: {
      stack: String(stack)
        .split("\n")
        .slice(0, 2)
        .map((line) => line.replace(/\([^()]*:\d+:\d+\)$/, "(<src>:<n>:<n>)")),
      stackTraceLimit: Error.stackTraceLimit,
      messages: {
        nullProperty: thrown(() => null.value),
        notFunction: thrown(() => {
          const value = {};
          value.missing();
        }),
        // oxlint-disable-next-line unicorn/no-new-array
        arrayLength: thrown(() => new Array(-1)),
        json: thrown(() => JSON.parse("{")),
        bigint: thrown(() => BigInt(1.5)),
        symbol: thrown(() => `${Symbol("s")}`),
        constAssign: thrown(() => {
          const fixed = 1;
          // oxlint-disable-next-line no-const-assign
          fixed = 2;
          return fixed;
        }),
      },
      functionSource: String(function sample(a, b) {
        return a + b;
      }),
      nativeSource: String(Math.max),
    },
    features: {
      builtins: Object.fromEntries(builtins.map((name) => [name, typeof path(name)])),
      css: Object.fromEntries(css.map((rule) => [rule, attempt(() => CSS.supports(rule))])),
      chrome: globalThis.chrome ? Object.keys(globalThis.chrome).sort() : null,
      prototypes: {
        Array: names(Array.prototype),
        String: names(String.prototype),
        Object: names(Object),
        Math: names(Math),
        Promise: names(Promise),
        Intl: names(globalThis.Intl),
        Iterator: names(globalThis.Iterator && Iterator.prototype),
        Navigator: names(globalThis.Navigator && Navigator.prototype),
      },
      globals: names(globalThis),
    },
  };
})();
