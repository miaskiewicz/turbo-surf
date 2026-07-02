// Type surface of the turbo-surf native (Rust) addon. JSON-returning functions
// are typed as `string` (the caller `JSON.parse`s); structured types live in the
// JS shim that wraps this addon.

export function version(): string;

/** Fetch a URL → JSON `{ html, finalUrl, status, redirected }`. */
export function fetchHtml(url: string): Promise<string>;
/** Fetch a URL as raw bytes → a Buffer (no charset decode). For binary
 * resources — images to pass to `screenshotWithAssets`. */
export function fetchBytes(url: string): Promise<Buffer>;

/** Crawl from a JSON options string → JSON array of page records. */
export function crawl(optsJson: string): Promise<string>;

/** Evaluate JS against the page DOM → result as a string (no event loop). */
export function evaluate(html: string, script: string): string;

/** Run the page's own JS (promises/timers/fetch/cookies) → hydrated HTML. */
export function render(html: string, baseUrl: string, script: string): string;

/** Hydrate by running the page's OWN scripts (inline + dynamically-injected chunks)
 * the way a browser does — fetch+execute each, fire onload, drain to quiescence — so
 * a real SPA bundle mounts. Async (Promise): must not block Node's event loop, since
 * chunk fetches may hit a same-process server. */
export function hydrate(
  html: string,
  baseUrl: string,
  cookies?: string,
  userAgent?: string,
): Promise<string>;

/** Live session: hydrate + keep the app's JS isolate ALIVE so interactions dispatch
 * real DOM events into the running app. Returns a session id. */
export function liveOpen(
  html: string,
  baseUrl: string,
  cookies?: string,
  userAgent?: string,
): Promise<number>;
/** Run JS in a live session, drain to quiescence, return String(globalThis.__RESULT). */
export function liveEval(id: number, script: string): Promise<string>;
/** Serialize the live session's current DOM to HTML. */
export function liveSerialize(id: number): string;
/** The live session's cookies (storageState JSON), incl. HttpOnly session cookies. */
export function liveCookies(id: number): string;
/** Close a live session (reset binding, drop isolate, unregister). */
export function liveClose(id: number): void;

/** Transform TS/JSX source → classic JS (swc). */
export function transform(src: string, ts: boolean, jsx: boolean): string;
/** Transform a TS/JSX bundle then render it → hydrated HTML. */
export function renderTs(
  html: string,
  baseUrl: string,
  src: string,
  ts: boolean,
  jsx: boolean,
): string;

/** Fetch with an explicit method/body (POST form submit). */
export function request(url: string, method: string, body?: string): Promise<string>;

/** Fetch carrying storageState cookies in, updated state out (persistence).
 * `headers` is a JSON object of extra request headers (setExtraHTTPHeaders). */
export function fetchWithCookies(
  url: string,
  cookies: string,
  method?: string,
  body?: string,
  headers?: string,
): Promise<string>;

// Actions by selector — mutate the DOM and return the new HTML.
export function fill(html: string, selector: string, value: string): string;
export function setChecked(html: string, selector: string, on: boolean): string;
export function selectOption(html: string, selector: string, value: string): string;
/** Click intent → JSON {action:"navigate"|"submit"|"inert", ...}. */
export function click(html: string, selector: string, baseUrl: string): string;

// Actions by node handle (back locator-scoped actions).
export function fillNode(html: string, node: number, value: string): string;
export function setCheckedNode(html: string, node: number, on: boolean): string;
export function selectOptionNode(html: string, node: number, value: string): string;
export function clickNode(html: string, node: number, baseUrl: string): string;

// Per-element accessors by node handle.
export function attrOf(html: string, node: number, name: string): string | null;
export function inputValueOf(html: string, node: number): string;
export function isVisible(html: string, node: number): boolean;
export function isChecked(html: string, node: number): boolean;
export function isEnabled(html: string, node: number): boolean;
export function isEditable(html: string, node: number): boolean;
export function isEmpty(html: string, node: number): boolean;
export function ariaRoleOf(html: string, node: number): string;
export function accessibleNameOf(html: string, node: number): string;
export function accessibleDescriptionOf(html: string, node: number): string;
export function selectedValuesOf(html: string, node: number): string[];
export function cssValueOf(html: string, node: number, name: string): string;
export function matchesAriaSnapshot(html: string, node: number, expected: string): boolean;
/** One-crossing batch read of a node's state — JSON
 * `{visible,checked,enabled,editable,empty,text,value,role,name,description}`. */
export function nodeSnapshot(html: string, node: number): string;

export function markdown(html: string, baseUrl: string): string;
/** Synthetic screenshot (no browser): native layout + paint of `html` at the
 * given (or default 1280×800) viewport. `screenshot` → PNG bytes (Buffer);
 * `screenshotSvg` → an SVG document string. */
export function screenshot(html: string, width?: number, height?: number): Buffer;
export function screenshotSvg(html: string, width?: number, height?: number): string;
/** `href`s of the page's `<link rel="stylesheet">` elements (verbatim). Resolve
 * against the page URL + fetch, then pass the concatenated CSS to `*WithCss`. */
export function stylesheetHrefs(html: string): string[];
/** Like `screenshot`/`screenshotSvg` but with caller-fetched external `<link>`
 * stylesheet CSS cascaded on top of the page's inline styles. */
export function screenshotWithCss(html: string, externalCss: string, width?: number, height?: number): Buffer;
export function screenshotSvgWithCss(html: string, externalCss: string, width?: number, height?: number): string;
/** Image refs in the page (`<img src>` + `background-image: url(...)`), verbatim
 * + de-duplicated (`data:` skipped). Resolve each against the page URL, fetch
 * the bytes, and pass them as the `images` map (ref → bytes) to `*WithAssets`. */
export function imageUrls(html: string): string[];
/** Like `*WithCss` but also paints `<img>`/`background-image` boxes from the
 * `images` map (each `imageUrls` ref → its fetched bytes). PNG/JPEG are drawn;
 * other formats fall back to a placeholder. */
export function screenshotWithAssets(html: string, externalCss: string, images: Record<string, Buffer>, width?: number, height?: number, fullPage?: boolean, systemFonts?: boolean): Buffer;
export function screenshotSvgWithAssets(html: string, externalCss: string, images: Record<string, Buffer>, width?: number, height?: number, fullPage?: boolean, systemFonts?: boolean): string;
export function text(html: string): string;
export function title(html: string): string;
export function html(html: string): string;
export function links(html: string, baseUrl: string): string[];

/** JSON-encoded results. */
export function interactiveElements(html: string, baseUrl: string): string;
export function accessibilityTree(html: string): string;
export function ariaSnapshot(html: string): string;
export function hydrationState(html: string): string;
export function detect(html: string): string;
export function query(html: string, selector: string, kind?: string): string;
export function extract(html: string, baseUrl: string, schemaJson: string): string;
