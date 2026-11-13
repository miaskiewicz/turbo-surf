// Type surface of the turbo-surf native (Rust) addon. JSON-returning functions
// are typed as `string` (the caller `JSON.parse`s); structured types live in the
// JS shim that wraps this addon.

export function version(): string;
/** Infer an uploaded file path's MIME type; unknown extensions use application/octet-stream. */
export function fileMimeType(path: string): string;
/** Actual V8 version linked into the native renderer; host-only diagnostics. */
export function engineVersion(): string;

/** Fetch a URL → JSON `{ html, finalUrl, status, redirected }`. */
export function fetchHtml(url: string): Promise<string>;
/** Fetch a URL as raw bytes → a Buffer (no charset decode). For binary
 * resources — images to pass to `screenshotWithAssets`. */
export function fetchBytes(
  url: string,
  requestStateId?: number,
  sourceUrl?: string,
  cookies?: string,
): Promise<Buffer>;
/** Create/close origin-scoped client-hint state shared by a browsing context. */
/** Derive a tab sharing a parent's local storage, HTTP cache and client hints. */
export function requestStateNew(parentContextId?: number): number;
export function requestStateNewApi(parentContextId?: number): number;
export function requestStateSetOffline(id: number, offline: boolean): void;
export function requestStateClose(id: number): void;
/** Replace host-owned per-tab browser emulation options. */
export function requestStateConfigure(id: number, optionsJson: string): void;
export function requestStateImportStorage(id: number, originsJson: string): void;
export function requestStateStorage(id: number): string;

/** Crawl from a JSON options string → JSON array of page records. */
export function crawl(optsJson: string): Promise<string>;

/** Evaluate JS against the page DOM → result as a string (no event loop). */
export function evaluate(html: string, script: string): string;
export function evaluateEngine(html: string, script: string): string;

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
  requestStateId?: number,
  documentResponse?: string,
): Promise<string>;

/** documentResponse is host JSON {status, referrer, headers: [[name, value], ...], viewport?: [width, height], hostNetworkLog?: number}.
 * initScript runs in the guest global before authored scripts.
 * engineInitScript is an optional driver-only body with the private __tsNs namespace.
 * realTimeTimers waits for wall-clock deadlines; omitted/false keeps crawler virtual time.
 * Live session: hydrate + keep the app's JS isolate ALIVE so interactions dispatch
 * real DOM events into the running app. Returns a session id. */
export function liveOpen(
  html: string,
  baseUrl: string,
  cookies?: string,
  userAgent?: string,
  requestStateId?: number,
  documentResponse?: string,
  initScript?: string,
  timeoutMs?: number,
  maxResourceBytes?: number,
  engineInitScript?: string,
  realTimeTimers?: boolean,
): Promise<number>;
/** Run JS in a live session, drain to quiescence, return String(globalThis.__RESULT). */
export function liveEval(id: number, script: string): Promise<string>;
/** Private-namespace driver eval; collect polls one ready turn without resuming hydration. */
export function liveEvalEngine(id: number, script: string, collect?: boolean): Promise<string>;
/** Bounded synchronous engine query; JSON-encode its return without draining the event loop. */
export function liveReadEngine(id: number, script: string): string;
/** Same non-pumping query, yielding Node while the native actor is busy. */
export function liveReadEngineAsync(id: number, script: string): Promise<string>;
/** Await an expression’s actual native Promise completion. */
export function liveEvalEngineExpression(id: number, expression: string): Promise<string>;
export function liveEvalExpression(id: number, expression: string): Promise<string>;
/** Collector value plus authoritative driver snapshot invalidation. */
export function liveEvalExpressionTracked(id: number, expression: string): Promise<string>;
/** Reserve the host transport queue before initial hydration. */
export function liveNetworkLogCreate(): number;
/** Drain {entries, revision} without entering a body-awaiting isolate. */
export function liveNetworkLog(id: number): Promise<string>;
/** Wait for a transport event or an explicit pump wake; returns {wake, closed}. */
export function liveNetworkLogWait(id: number, wakeToken: string): Promise<string>;
export function liveNetworkLogWake(id: number): void;
/** Discard unobserved console handles at the next actor boundary. */
export function liveNetworkLogDiscardHandles(id: number, ids: string): void;
/** Acknowledge a batch only after the host has processed its entries. */
export function liveNetworkLogAcknowledge(id: number, revision: string): void;
/** Serialize the live session's current DOM to HTML. */
export function liveSerialize(id: number): string;
/** The live session's cookies (storageState JSON), incl. HttpOnly session cookies. */
export function liveCookies(id: number): string;
export function liveSetCookies(id: number, cookies: string): void;
/** Close this actual target; final actor close returns its trusted lifecycle snapshot JSON. */
export function liveClose(id: number): string | null;

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
  bypassConsent?: boolean,
  requestStateId?: number,
  contextJson?: string,
  bodyBytes?: Buffer,
  maxRedirects?: number,
  noCache?: boolean,
  timeoutMs?: number,
  maxBytes?: number,
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
export function textContentOf(html: string, node: number): string;
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
export function screenshotWithCss(
  html: string,
  externalCss: string,
  width?: number,
  height?: number,
): Buffer;
export function screenshotSvgWithCss(
  html: string,
  externalCss: string,
  width?: number,
  height?: number,
): string;
/** Image refs in the page (`<img src>` + `background-image: url(...)`), verbatim
 * + de-duplicated (`data:` skipped). Resolve each against the page URL, fetch
 * the bytes, and pass them as the `images` map (ref → bytes) to `*WithAssets`. */
export function imageUrls(html: string): string[];
/** Re-encode native PNG output as JPEG with quality 0..100 (default 80). */
export function screenshotJpeg(png: Buffer, quality?: number): Buffer;
/** Crop rendered PNG pixels without relaying out the document. */
export function screenshotCrop(
  png: Buffer,
  x: number,
  y: number,
  width: number,
  height: number,
): Buffer;
/** Like `*WithCss` but also paints `<img>`/`background-image` boxes from the
 * `images` map (each `imageUrls` ref → its fetched bytes). PNG/JPEG are drawn;
 * other formats fall back to a placeholder. */
export function screenshotWithAssets(
  html: string,
  externalCss: string,
  images: Record<string, Buffer>,
  width?: number,
  height?: number,
  fullPage?: boolean,
  systemFonts?: boolean,
): Buffer;
export function screenshotSvgWithAssets(
  html: string,
  externalCss: string,
  images: Record<string, Buffer>,
  width?: number,
  height?: number,
  fullPage?: boolean,
  systemFonts?: boolean,
): string;
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
export function queryCount(html: string, selector: string, kind?: string): number;
export function query(html: string, selector: string, kind?: string, root?: number): string;
export function getBy(
  html: string,
  kind: string,
  value: string,
  name?: string,
  root?: string,
  options?: string,
): string;
export function extract(html: string, baseUrl: string, schemaJson: string): string;

/** Resize trusted live geometry and deliver resize/media rendering updates. */
export function liveResize(id: number, width: number, height: number): Promise<string>;

/** Conservative pixel-resource selection using independent intrinsic metadata. */
export function screenshotCaptureResources(
  html: string,
  css: string,
  width: number,
  height: number,
  metadataJson: string,
  optionsJson: string,
): string;
/** Raster capture with independent metadata and explicit paint options. */
export function screenshotCapture(
  html: string,
  css: string,
  images: Record<string, Buffer>,
  width?: number,
  height?: number,
  optionsJson?: string,
  metadataJson?: string,
): Buffer;
export function screenshotCaptureSvg(
  html: string,
  css: string,
  images: Record<string, Buffer>,
  width?: number,
  height?: number,
  optionsJson?: string,
  metadataJson?: string,
): string;

/** An owned immutable layout/image snapshot. Close it to release retained resources. */
export class ScreenshotScene {
  constructor(
    html: string,
    css: string,
    images: Record<string, Buffer>,
    width: number,
    height: number,
    optionsJson?: string,
    metadataJson?: string,
  );
  readonly paintCount: number;
  readonly closed: boolean;
  png(): Buffer;
  svg(): string;
  close(): void;
}
export function liveSetContent(
  id: number,
  html: string,
  waitUntil?: string,
  timeoutMs?: number,
): Promise<string>;

export function liveNetworkLogPublishIntercepted(id: number, header: string, body: Buffer): void;

/** One lifecycle consumer per actor; target IDs are trusted decimal strings. */
export function liveAuxiliaryState(id: number): string;
/** Idempotently alias an existing producer target in the same actor. */
export function liveAuxiliaryAdopt(id: number, target: string): number;
/** Resolve with JSON {closed:boolean,finalState?:object}; finalState retains the final actual lifecycle events after actor removal. */
export function liveAuxiliaryWait(id: number, revision?: string): Promise<string>;
/** Close every target and release the shared actor; return final lifecycle snapshot JSON, or null if already gone. */
export function liveActorClose(id: number): string | null;

/** Navigate the alias' actual target inside its existing actor. */
export function liveNavigateTarget(id: number, url: string, history?: "push" | "replace" | "back" | "forward", optionsJson?: string): Promise<string>;

/** Replace context-only bootstrap for later auxiliary documents; no guest evaluation. */
export function liveAuxiliaryConfigure(actor: number, initScript: string, engineInitScript: string): Promise<void>;
/** Replace Page-only bootstrap for this actual target's own later navigation. */
export function liveTargetConfigure(id: number, initScript: string, engineInitScript: string): Promise<void>;
