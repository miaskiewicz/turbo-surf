//! Render-tier integration tests. These drive deno_core (which boots V8 its own
//! way), so they live in a SEPARATE test binary from the lib unit tests — the
//! vendored `browser_env_upstream.rs` smoke test boots a standalone V8 platform in
//! the lib binary, and the two platform initializations must not share a process.

use turbo_surf_render::{
    render_html, render_html_async, render_hydrate, render_hydrate_with_budget, render_page,
    render_page_pooled, render_page_with_budget, run_with_dom, PageSession,
    DEFAULT_RENDER_BUDGET_MS, SCRIPT_BOUNDARY,
};

// --- per-script isolation: a throw in one <script> doesn't abort the rest -----
// A browser runs each <script> as a separate top-level program: an uncaught error in
// one does NOT stop later scripts, and top-level let/const/function/var still populate
// the shared realm scope so a later script sees them. The render tier splits a
// page-script bundle on SCRIPT_BOUNDARY and runs each part on its own.
#[tokio::test]
async fn a_throwing_script_does_not_abort_later_scripts() {
    let base = "http://localhost/";
    let html = "<body><div id='o'></div></body>";
    // Part 1 throws (simulates google's `_._DumpException` cascade); part 2 declares a
    // top-level `let` + `function`; part 3 reads them and writes the DOM — so passing
    // proves both isolation (part 3 ran) AND shared realm scope (it saw part 2's decls).
    let bundle = [
        r#"throw new Error("boom from script 1");"#,
        r#"let PART2 = "ran"; function greet(){ return "hi:" + PART2; }"#,
        r#"document.getElementById('o').textContent = greet();"#,
    ]
    .join(SCRIPT_BOUNDARY);

    let out = render_page(html, base, &bundle).await.unwrap();
    assert!(
        out.contains("hi:ran"),
        "later scripts run past a throw and share realm scope: {out}"
    );
}

// set_fingerprint is a PROCESS-global override; serialize the tests that read or
// mutate it so a parallel override can't leak into a default-assuming test.
static FP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// --- pooled render (reused isolate) keeps fresh-navigation isolation --------
// `render_page_pooled` reuses one V8 isolate across pages for speed; the cross-page
// global scrub must make a reused isolate behave like a fresh navigation. Run a page
// that leaks `window` globals, then a page that reads them — interleaved with the
// fresh `render_page` to prove byte parity and no A→B contamination.
#[tokio::test]
async fn pooled_render_scrubs_cross_page_globals() {
    let base = "http://localhost/";
    let leak = (
        "<body><div id='o'></div></body>",
        r#"window.LEAK = "A"; document.getElementById('o').textContent = "A:" + (window.SEEN || "none"); window.SEEN = "fromA";"#,
    );
    let reader = (
        "<body><div id='o'></div></body>",
        r#"document.getElementById('o').textContent = "B:" + (window.LEAK || "clean") + ":" + (window.SEEN || "none");"#,
    );

    let fresh_leak = render_page(leak.0, base, leak.1).await.unwrap();
    let fresh_reader = render_page(reader.0, base, reader.1).await.unwrap();
    assert!(
        fresh_reader.contains("B:clean:none"),
        "fresh reader sees no leaked globals: {fresh_reader}"
    );

    // Interleave through the pooled path; each page must match its fresh render.
    for i in 0..3 {
        let a = render_page_pooled(leak.0, base, leak.1, DEFAULT_RENDER_BUDGET_MS)
            .await
            .unwrap();
        let b = render_page_pooled(reader.0, base, reader.1, DEFAULT_RENDER_BUDGET_MS)
            .await
            .unwrap();
        assert_eq!(a, fresh_leak, "pooled leak page parity, iter {i}");
        assert_eq!(b, fresh_reader, "pooled reader saw A's globals, iter {i}");
    }
}

// --- reads over a real parsed DOM -------------------------------------------
#[test]
fn reads_real_parsed_dom() {
    assert_eq!(
        run_with_dom(
            "<html><body><h1 id='title'>Hello</h1></body></html>",
            "document.querySelector('h1').textContent"
        )
        .unwrap(),
        "Hello"
    );
    assert_eq!(
        run_with_dom(
            "<html><body><h1 id='title'>Hello</h1></body></html>",
            "document.getElementById('title').getAttribute('id')"
        )
        .unwrap(),
        "title"
    );
}

#[test]
fn query_selector_all_returns_list() {
    let n = run_with_dom(
        "<ul><li>a</li><li>b</li><li>c</li></ul>",
        "String(document.querySelectorAll('li').length)",
    )
    .unwrap();
    assert_eq!(n, "3");
}

#[test]
fn scoped_query_within_element() {
    let txt = run_with_dom(
        "<div id='a'><span class='x'>1</span></div><span class='x'>2</span>",
        "document.getElementById('a').querySelector('.x').textContent",
    )
    .unwrap();
    assert_eq!(txt, "1");
}

#[test]
fn element_api_surface() {
    // tagName / innerHTML / outerHTML / body / mutate + reserialize, all within one
    // render (the binding holds the tree for the life of the install).
    let out = run_with_dom(
        "<html><body><div id='a' class='c'>hi</div></body></html>",
        r#"
        const el = document.querySelector('#a');
        el.setAttribute('data-x', '1');
        el.id = 'b';
        JSON.stringify({
          tag: el.tagName,
          inner: el.innerHTML,
          outer: el.outerHTML,
          hasBody: document.body !== null,
          dataX: el.getAttribute('data-x'),
          byNewId: document.getElementById('b') !== null,
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["tag"], "DIV", "{out}");
    assert!(v["inner"].as_str().unwrap().contains("hi"), "{out}");
    assert!(
        v["outer"].as_str().unwrap().contains("class=\"c\""),
        "{out}"
    );
    assert_eq!(v["hasBody"], true, "{out}");
    assert_eq!(v["dataX"], "1", "{out}");
    assert_eq!(v["byNewId"], true, "{out}");
}

#[test]
fn window_and_navigator_present() {
    assert_eq!(
        run_with_dom("<body></body>", "String(window === globalThis)").unwrap(),
        "true"
    );
}

// Page JS that profiles the browser must see a coherent real-Chrome navigator,
// not the old `turbo-surf`/`turbo-test` tell — the no-Chromium env emulation that
// gets past consistency-only anti-bot gates (see ENV_BOOTSTRAP in runtime.rs).
#[test]
fn navigator_looks_like_chrome() {
    let _g = FP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    turbo_surf_render::set_fingerprint("{}"); // defaults (render_html = fresh isolate)
                                              // Dump the identity surface into a DOM attribute so one fresh render asserts
                                              // it all deterministically (run_with_dom caches its isolate, so it can't
                                              // observe the current override).
    let probe = "document.body.setAttribute('data-n', [\
        navigator.userAgent.includes('Chrome/') && navigator.userAgent.includes('Macintosh'),\
        navigator.platform, navigator.vendor, navigator.webdriver,\
        navigator.plugins.length > 0, typeof window.chrome,\
        navigator.connection.effectiveType, navigator.userAgentData.platform,\
        typeof fetch.toString].join('|'))";
    let out = turbo_surf_render::render_html("<body></body>", probe).unwrap();
    assert!(
        out.contains("data-n=\"true|MacIntel|Google Inc.|false|true|object|4g|macOS|function\""),
        "navigator not coherent Chrome: {out}"
    );
}

// Runtime fingerprint override: every navigator field has a default and is
// controllable via set_fingerprint (the MCP `set_fingerprint` tool). Uses
// render_html (a fresh isolate per call, so ENV_BOOTSTRAP re-reads the override),
// writing the values into the DOM to assert on the hydrated output.
#[test]
fn fingerprint_override_applies_and_resets() {
    let _g = FP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let probe = "document.body.setAttribute('data-fp', \
        navigator.platform + '|' + navigator.hardwareConcurrency + '|' + \
        navigator.languages.join(',') + '|' + screen.width + '|' + \
        navigator.userAgentData.brands[0].version)";

    turbo_surf_render::set_fingerprint(
        r#"{"platform":"Win32","hardwareConcurrency":16,"languages":["en-GB","en"],
            "screen":{"width":2560,"height":1440},"chromeMajor":150}"#,
    );
    let out = turbo_surf_render::render_html("<body></body>", probe).unwrap();
    assert!(
        out.contains("data-fp=\"Win32|16|en-GB,en|2560|150\""),
        "override not applied: {out}"
    );

    // Reset → Chrome 149 macOS defaults return.
    turbo_surf_render::set_fingerprint("{}");
    let out = turbo_surf_render::render_html("<body></body>", probe).unwrap();
    assert!(
        out.contains("data-fp=\"MacIntel|8|en-US,en|1920|149\""),
        "reset to defaults failed: {out}"
    );
}

// Native-function fidelity: page JS that does `fetch.toString()` (a common
// anti-bot probe) must see "[native code]", not our polyfill's JS source — and
// the toString trap must report itself native too.
#[test]
fn shimmed_builtins_report_native() {
    assert_eq!(
        run_with_dom("<body></body>", "fetch.toString()").unwrap(),
        "function fetch() { [native code] }"
    );
    assert_eq!(
        run_with_dom("<body></body>", "setTimeout.toString()").unwrap(),
        "function setTimeout() { [native code] }"
    );
    assert_eq!(
        run_with_dom("<body></body>", "Function.prototype.toString.toString()").unwrap(),
        "function toString() { [native code] }"
    );
    // A genuine page function must still reveal its JS source (we only mask shims).
    assert_eq!(
        run_with_dom("<body></body>", "(function foo(){return 1}).toString()").unwrap(),
        "function foo(){return 1}"
    );
}

// Setting `location.href` must DECOMPOSE the URL into pathname/search/hash/host/etc.
// Next's app-router reads usePathname()/useSearchParams() off these; a static location
// (browser_env's plain object) left pathname at "/" so the payroll login route guard saw
// a protected route and rendered "Redirecting…" instead of the form.
#[test]
fn location_href_decomposes_into_components() {
    let out = run_with_dom(
        "<body></body>",
        r#"
        location.href = "https://app.example/login?next=%2Fhome&x=1#frag";
        JSON.stringify({
          pathname: location.pathname, search: location.search, hash: location.hash,
          host: location.host, hostname: location.hostname, protocol: location.protocol,
          origin: location.origin,
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pathname"], "/login", "{out}");
    assert_eq!(v["search"], "?next=%2Fhome&x=1", "{out}");
    assert_eq!(v["hash"], "#frag", "{out}");
    assert_eq!(v["host"], "app.example", "{out}");
    assert_eq!(v["protocol"], "https:", "{out}");
    assert_eq!(v["origin"], "https://app.example", "{out}");
}

// FormData — PropelAuth + form libs build credential payloads with it during hydration;
// deno_core ships none, so the login bundle crashed with "FormData is not defined".
#[test]
fn form_data_present_and_spec_shaped() {
    let out = run_with_dom(
        "<body></body>",
        r#"
        const fd = new FormData();
        fd.append("a", "1"); fd.append("a", "2"); fd.set("b", "3"); fd.set("a", "9");
        JSON.stringify({
          a: fd.getAll("a"), b: fd.get("b"), hasB: fd.has("b"),
          entries: [...fd.entries()],
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["a"],
        serde_json::json!(["9"]),
        "set must replace all: {out}"
    );
    assert_eq!(v["b"], "3", "{out}");
    assert_eq!(v["hasB"], true, "{out}");
}

// The interactive-SPA capability: a LIVE PageSession keeps the hydrated app's JS alive,
// so a dispatched event re-enters a delegated listener (the way React 17+ delegates all
// events at the root) and the re-render is observable — none of which works on the
// stateless render_* paths (they serialize + reset, killing the running app). This is
// the foundation for driving an authenticated SPA login through the shim.
#[tokio::test]
async fn live_session_dispatches_events_into_running_app() {
    // A root-delegated click listener (React-style): one listener on `document`, the
    // handler reads event.target and re-renders. Clicking the (deep) button must bubble
    // up to it. Also a controlled-input-style `input` listener mirroring value → text.
    let html = r#"<body>
      <div id="app">
        <button id="btn">+</button>
        <span data-test-id="count">0</span>
        <input id="field" />
        <span data-test-id="echo"></span>
      </div>
      <script>
        let n = 0;
        document.addEventListener('click', (e) => {
          if (e.target && e.target.id === 'btn') {
            n++;
            document.querySelector('[data-test-id="count"]').textContent = String(n);
          }
        });
        document.addEventListener('input', (e) => {
          if (e.target && e.target.id === 'field') {
            document.querySelector('[data-test-id="echo"]').textContent = e.target.value;
          }
        });
      </script>
    </body>"#;

    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    // Dispatch a bubbling click on the deep button — must reach the document listener.
    let click = r#"document.getElementById('btn').dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true }));"#;
    session.eval(click).await.unwrap();
    assert!(
        session.serialize().contains(r#"data-test-id="count">1<"#),
        "delegated click handler must run + re-render: {}",
        session.serialize()
    );

    // State persists across ops — a second click increments again (proves the app is
    // alive between calls, not re-hydrated from a string each time).
    session.eval(click).await.unwrap();
    assert!(
        session.serialize().contains(r#"data-test-id="count">2<"#),
        "running app state must persist across ops: {}",
        session.serialize()
    );

    // Controlled-input: set value + dispatch `input` → the delegated handler mirrors it.
    let fill = r#"const f = document.getElementById('field'); f.value = 'hello';
        f.dispatchEvent(new InputEvent('input', { bubbles: true }));"#;
    session.eval(fill).await.unwrap();
    assert!(
        session
            .serialize()
            .contains(r#"data-test-id="echo">hello<"#),
        "input event must reach the controlled-input handler: {}",
        session.serialize()
    );

    session.close();
}

// RSC soft-nav must preserve the target's QUERY STRING. Next App Router client
// navigation (`router.push('/x?employeeIds=42')`) fetches the target's RSC flight with
// an `RSC` header; we record the target on `__rscNav` and the live-session driver hard-
// reloads it. Recording only `u.pathname` dropped the query, so the off-cycle termination
// flow (which passes the selected employee as `?employeeIds=`) landed on the bare route and
// `waitForURL(/…\/termination\?employeeIds=/)` never matched. The recorded target must keep
// the app query but STRIP Next's internal `_rsc` cache-buster (a hard reload carrying `_rsc`
// returns a flight payload, not HTML).
#[tokio::test]
async fn rsc_soft_nav_preserves_query_and_strips_rsc_param() {
    let html = r#"<body><div id="app">people</div></body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/entity/x/admin/people/active",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    let nav = session
        .eval(
            r#"globalThis.__rscNav = '';
        globalThis.fetch('/entity/x/admin/payroll/off-cycle/new/termination?employeeIds=42&_rsc=abc123',
            { headers: { RSC: '1' } }).catch(() => {});
        globalThis.__RESULT = globalThis.__rscNav || '';"#,
        )
        .await
        .unwrap();
    assert_eq!(
        nav, "/entity/x/admin/payroll/off-cycle/new/termination?employeeIds=42",
        "__rscNav must preserve the app query string (employeeIds) and drop Next's _rsc param"
    );

    session.close();
}

// __tcResolveScoped must apply a Locator.filter({hasNotText}) BEFORE indexing, so
// `cards.filter({hasNotText: x}).first().getByTestId('y')` scopes the child to the SAME
// element the static read path picks. The pay-schedule delete-409 guard does exactly this
// (target a SEEDED card by filtering OUT the just-created one); without filter-in-scope the
// child resolved against the unfiltered set → the toggle click landed on the wrong card and
// its delete button never revealed.
#[tokio::test]
async fn scoped_resolve_applies_filter_before_indexing() {
    let html = r#"<body>
      <div class="card"><span>alpha cycle</span><button data-test-id="del">x</button></div>
      <div class="card"><span>beta seeded</span><button data-test-id="del">x</button></div>
    </body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    // Scope: `.card` filtered to NOT contain "alpha cycle", first → the beta card; leaf = its
    // del button. The resolved element's parent card text must be the BETA card's.
    let card_text = session
        .eval(
            r#"globalThis.__tcResolveScoped(
            [{ sel: '.card', idx: 0, filter: { hasNotText: 'alpha cycle' } }],
            { selector: '[data-test-id="del"]' });
        const arr = JSON.parse(globalThis.__RESULT);
        const all = document.querySelectorAll('*');
        const el = arr.length ? all[arr[0].idx] : null;
        globalThis.__RESULT = el ? el.parentNode.textContent : 'NONE';"#,
        )
        .await
        .unwrap();
    assert!(
        card_text.contains("beta seeded") && !card_text.contains("alpha"),
        "filtered scope must resolve the del button inside the BETA card, got: {card_text}"
    );

    session.close();
}

// Mirrors the payroll /login hydration crash: the page's client JS (Next.js +
// the PropelAuth SDK) uses WHATWG `URL` / `URLSearchParams` while building the
// login form. deno_core doesn't ship those globals, so hydration died with
// "ReferenceError: URL is not defined" and the `login-email-input` never
// rendered. The render tier must provide them.
#[test]
fn whatwg_url_available_for_hydration() {
    let html = render_html(
        "<body><div id='root'></div></body>",
        r#"
        const u = new URL("https://app.example/login?next=%2Fhome");
        const sp = new URLSearchParams(u.search);
        const root = document.getElementById('root');
        const input = document.createElement('input');
        input.setAttribute('data-testid', 'login-email-input');
        input.setAttribute('data-next', sp.get('next'));
        input.setAttribute('data-host', u.hostname);
        input.setAttribute('data-proto', u.protocol);
        root.appendChild(input);
        "#,
    )
    .unwrap();
    assert!(
        html.contains(r#"data-testid="login-email-input""#),
        "login form should render after hydration: {html}"
    );
    assert!(
        html.contains(r#"data-next="/home""#),
        "URLSearchParams.get must decode: {html}"
    );
    assert!(
        html.contains(r#"data-host="app.example""#),
        "URL.hostname must parse: {html}"
    );
    assert!(
        html.contains(r#"data-proto="https:""#),
        "URL.protocol must parse: {html}"
    );
}

// document.createTreeWalker — MUI's DataGrid / focus-trap walks the tree with it; its
// absence threw "createTreeWalker is not a function" and blanked the whole page.
#[test]
fn create_tree_walker_walks_the_dom() {
    let out = run_with_dom(
        "<body><div id='r'><a id='a1'>1</a><span><a id='a2'>2</a></span><a id='a3'>3</a></div></body>",
        r#"
        const root = document.getElementById('r');
        const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT, {
          acceptNode: (n) => n.tagName === 'A' ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP,
        });
        const ids = [];
        let n; while ((n = w.nextNode())) ids.push(n.getAttribute('id'));
        ids.join(',');
        "#,
    )
    .unwrap();
    assert_eq!(
        out, "a1,a2,a3",
        "TreeWalker must visit accepted <a> in document order"
    );
}

// Next.js's webpack runtime resolves chunk paths via `document.currentScript`
// (getPathFromScript → currentScript.getAttribute(...)). Our tier ran scripts as
// one blob with no currentScript, so hydration crashed with "Cannot read
// properties of undefined (reading 'getAttribute')". currentScript must be a real
// element so that read is safe.
#[test]
fn document_current_script_is_present_for_webpack() {
    let out = run_with_dom(
        "<body><div id='root'></div></body>",
        r#"
        const cs = document.currentScript;
        // webpack does currentScript.getAttribute('src').replace(...) — must be a string.
        const src = cs && typeof cs.getAttribute === 'function' ? cs.getAttribute('src') : 'MISSING';
        const safe = typeof src === 'string' ? src.replace(/x/g, 'x') : 'NOT-A-STRING';
        document.getElementById('root').setAttribute('data-cs', safe);
        document.getElementById('root').getAttribute('data-cs');
        "#,
    )
    .unwrap();
    assert_eq!(
        out, "http://localhost/",
        "currentScript.getAttribute('src') must be the page URL string, got {out}"
    );
}

// Embeddable widgets (PropelAuth's login) render into a Shadow DOM via
// host.attachShadow(). rtdom has no shadow tree, so we fall back to the light DOM —
// attachShadow returns the host, content lands in the serialized document. Without
// it, hydration crashed with "attachShadow is not a function".
#[test]
fn attach_shadow_falls_back_to_light_dom() {
    let html = render_html(
        "<body><div id='host'></div></body>",
        r#"
        const host = document.createElement('div'); // React creates the shadow host
        document.getElementById('host').appendChild(host);
        const root = host.attachShadow({ mode: 'open' });
        // code reads shadowRoot.host to get the host back (Next devtools theme code)
        root.host.setAttribute('data-host-ok', '1');
        const input = document.createElement('input');
        input.setAttribute('data-testid', 'shadow-input');
        root.appendChild(input);
        "#,
    )
    .unwrap();
    assert!(
        html.contains(r#"data-testid="shadow-input""#),
        "content rendered into a shadow root must reach the serialized light DOM: {html}"
    );
    assert!(
        html.contains(r#"data-host-ok="1""#),
        "shadowRoot.host must point back to the host element: {html}"
    );
}

// The React Server Components client reads the flight payload as a ReadableStream,
// and fetch/abortable work needs AbortController. deno_core ships neither.
#[tokio::test]
async fn readable_stream_and_abort_controller_present() {
    let html = render_page(
        "<body><div id='root'></div></body>",
        "about:blank",
        r#"
        (async () => {
          const rs = new ReadableStream({
            start(c) { c.enqueue("alpha"); c.enqueue("beta"); c.close(); },
          });
          const reader = rs.getReader();
          let parts = [];
          for (;;) { const { value, done } = await reader.read(); if (done) break; parts.push(value); }
          const ac = new AbortController();
          const aborted0 = ac.signal.aborted;
          ac.abort();
          document.getElementById('root').setAttribute('data-parts', parts.join(','));
          document.getElementById('root').setAttribute('data-abort', String(aborted0) + '->' + String(ac.signal.aborted));
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(
        html.contains(r#"data-parts="alpha,beta""#),
        "ReadableStream reader must yield chunks: {html}"
    );
    assert!(
        html.contains(r#"data-abort="false->true""#),
        "AbortController.abort must flip signal: {html}"
    );
}

// RSC flight is a STREAMING producer: the controller is enqueued/closed LATER than the
// first read() (flight rows arrive as `__next_f` pushes; close fires on DOMContentLoaded).
// A read() that hits an empty-but-OPEN stream must PARK until the next enqueue/close —
// returning {done:true} there truncates the flight mid-stream and the render never
// converges. This drives the producer from a timer AFTER the reader has already parked.
#[tokio::test]
async fn readable_stream_parks_for_async_producer() {
    let html = render_page(
        "<body><div id='root'></div></body>",
        "about:blank",
        r#"
        (async () => {
          let ctrl;
          const rs = new ReadableStream({ start(c) { ctrl = c; } }); // empty at start
          // Producer runs on later turns, AFTER read() has parked on the empty stream.
          setTimeout(() => ctrl.enqueue("one"), 0);
          setTimeout(() => ctrl.enqueue("two"), 0);
          setTimeout(() => ctrl.close(), 0);
          const reader = rs.getReader();
          const parts = [];
          for (;;) { const { value, done } = await reader.read(); if (done) break; parts.push(value); }
          document.getElementById('root').setAttribute('data-parts', parts.join(','));
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(
        html.contains(r#"data-parts="one,two""#),
        "reader must park on an empty-open stream and resume on later enqueue/close, \
         not report EOF early: {html}"
    );
}

// File / Blob / FileReader / Headers — auth SDKs + analytics (PostHog) reference these
// during hydration; their absence aborted PostHog init ("File/Headers is not defined")
// and a fetch Response with no `headers` crashed Next's RSC client navigation
// (`res.headers.get('content-type')`). Verify the globals exist and the fetch Response
// exposes a working `headers`.
#[tokio::test]
async fn web_platform_globals_for_hydration() {
    let out = render_html(
        "<body><div id='root'></div></body>",
        r#"
        const fr = new FileReader();
        const f = new File(["hi"], "a.txt", { type: "text/plain" });
        const h = new Headers({ "Content-Type": "text/x-component" });
        const out = document.getElementById('root');
        out.setAttribute('data-file', String(f.name) + ":" + String(f.size));
        out.setAttribute('data-blob', String(new Blob(["abcd"]).size));
        out.setAttribute('data-hdr', String(h.get("content-type")));   // case-insensitive
        out.setAttribute('data-types', [typeof File, typeof Blob, typeof FileReader, typeof Headers].join(','));
        "#,
    )
    .unwrap();
    assert!(out.contains(r#"data-file="a.txt:2""#), "File: {out}");
    assert!(out.contains(r#"data-blob="4""#), "Blob.size: {out}");
    assert!(
        out.contains(r#"data-hdr="text/x-component""#),
        "Headers.get (ci): {out}"
    );
    assert!(
        out.contains(r#"data-types="function,function,function,function""#),
        "all four globals defined: {out}"
    );
}

// Constructable stylesheets (emotion/MUI) + the extra HTML*Element constructors the
// vendored browser_env's ctor list omits. A missing `CSSStyleSheet`/`HTMLDialogElement`
// reference aborted the chunk mid-hydration and blanked the whole tree.
#[tokio::test]
async fn constructable_stylesheet_and_extra_html_element_ctors() {
    let out = render_html(
        "<body><dialog id='d'></dialog><div id='root'></div></body>",
        r#"
        const sheet = new CSSStyleSheet();
        sheet.replaceSync("a{color:red}");
        sheet.insertRule("b{color:blue}", 0);
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
        const root = document.getElementById('root');
        root.setAttribute('data-rules', String(sheet.cssRules.length));
        root.setAttribute('data-adopted', String(document.adoptedStyleSheets.length));
        root.setAttribute('data-ctors', [typeof CSSStyleSheet, typeof HTMLDialogElement, typeof HTMLTableRowElement].join(','));
        // tag-keyed instanceof on a real node
        root.setAttribute('data-isdialog', String(document.getElementById('d') instanceof HTMLDialogElement));
        "#,
    )
    .unwrap();
    assert!(out.contains(r#"data-rules="2""#), "cssRules: {out}");
    assert!(
        out.contains(r#"data-adopted="1""#),
        "adoptedStyleSheets: {out}"
    );
    assert!(
        out.contains(r#"data-ctors="function,function,function""#),
        "ctors defined: {out}"
    );
    assert!(
        out.contains(r#"data-isdialog="true""#),
        "dialog node instanceof HTMLDialogElement: {out}"
    );
}

// crypto.subtle.digest (real SHA-256), BroadcastChannel, and WebSocket — auth SDKs
// (PropelAuth) + analytics use these during hydration. WebSocket must not hang/throw.
#[tokio::test]
async fn crypto_subtle_websocket_broadcastchannel_present() {
    let html = render_page(
        "<body><div id='root'></div></body>",
        "about:blank",
        r#"
        (async () => {
          const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("abc"));
          const hex = Array.from(new Uint8Array(buf)).map((b) => b.toString(16).padStart(2, "0")).join("");
          const root = document.getElementById("root");
          root.setAttribute("data-sha", hex);
          root.setAttribute("data-ws", typeof WebSocket);
          root.setAttribute("data-bc", typeof BroadcastChannel);
          const ws = new WebSocket("wss://x/y"); // must not throw or hang
          root.setAttribute("data-ws-state", String(ws.readyState));
          const bc = new BroadcastChannel("t"); let got = "";
          const bc2 = new BroadcastChannel("t"); bc2.onmessage = (e) => { got = e.data; root.setAttribute("data-bc-msg", got); };
          bc.postMessage("ping");
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(
        html.contains(
            r#"data-sha="ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad""#
        ),
        "SHA-256(\"abc\") must match the known vector: {html}"
    );
    assert!(
        html.contains(r#"data-ws="function""#) && html.contains(r#"data-ws-state="0""#),
        "WebSocket stub present + CONNECTING: {html}"
    );
    assert!(
        html.contains(r#"data-bc="function""#),
        "BroadcastChannel present: {html}"
    );
    assert!(
        html.contains(r#"data-bc-msg="ping""#),
        "BroadcastChannel delivers same-name messages: {html}"
    );
}

// Web globals deno_core lacks but app bundles use (TextEncoder/Decoder round-trip,
// crypto.getRandomValues, btoa/atob). A bundle touching any of these mid-hydration
// would otherwise crash with "X is not defined".
#[test]
fn encoding_crypto_base64_globals_present() {
    let out = run_with_dom(
        "<body></body>",
        r#"
        const enc = new TextEncoder().encode("héllo");
        const back = new TextDecoder().decode(enc);
        const rnd = new Uint8Array(4); crypto.getRandomValues(rnd);
        JSON.stringify({ roundtrip: back, bytes: enc.length, uuid: typeof crypto.randomUUID(), b64: btoa("hi"), un: atob(btoa("hi")) });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["roundtrip"], "héllo", "{out}");
    assert_eq!(v["bytes"], 6, "héllo is 6 UTF-8 bytes: {out}");
    assert_eq!(v["b64"], "aGk=", "{out}");
    assert_eq!(v["un"], "hi", "{out}");
}

// The `CSS` interface (CSS.supports / CSS.escape). Google's deferred `xjs` bundle — and
// feature-detecting UI/CSS-in-JS libraries generally — reference the `CSS` global at
// load; deno_core ships none, so the bundle aborted mid-hydration with
// "CSS is not defined" (seen on google.com). A minimal namespace with a working
// `supports` (parses the value shape, always "supported" headless) + spec `escape`.
#[test]
fn css_interface_global_present() {
    let out = run_with_dom(
        "<body></body>",
        r#"
        JSON.stringify({
          t: typeof CSS,
          fn: typeof CSS.supports,
          sup2: CSS.supports("display", "flex"),
          sup1: CSS.supports("(display: grid)"),
          esc: CSS.escape("a.b#c"),
          escLead: CSS.escape("1x"),
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["t"], "object", "window.CSS is a namespace object: {out}");
    assert_eq!(v["fn"], "function", "{out}");
    assert_eq!(v["sup2"], true, "{out}");
    assert_eq!(v["sup1"], true, "{out}");
    // CSS.escape escapes CSS-syntax chars; a leading digit is escaped as a hex code point.
    assert_eq!(v["esc"], "a\\.b\\#c", "{out}");
    assert_eq!(v["escLead"], "\\31 x", "{out}");
}

// XMLHttpRequest must expose the EventTarget surface (`addEventListener` / `removeEventListener`)
// like a real XHR — Google's home-page bundle wires its request via
// `req.addEventListener('load'/'readystatechange', …)` rather than the `onload` property, and
// our stub only had the `on*` props, so it crashed with
// "b.i.req.addEventListener is not a function" and aborted the module.
#[tokio::test]
async fn xhr_add_event_listener_fires_load() {
    let port = spawn_json_server(r#"{"ok":1}"#).await;
    let html = render_page(
        "<body><div id='root'>loading</div></body>",
        &base(port),
        r#"
        const xhr = new XMLHttpRequest();
        let states = 0;
        xhr.addEventListener('readystatechange', () => { states++; });
        xhr.addEventListener('load', () => {
          document.getElementById('root').textContent = 'loaded:' + xhr.status + ':' + (states > 0);
        });
        xhr.open('GET', '/data.json');
        xhr.send();
        "#,
    )
    .await
    .unwrap()
    .replace("&nbsp;", " ");
    assert!(
        html.contains("loaded:200:true"),
        "xhr.addEventListener('load') must fire + readystatechange listeners run: {html}"
    );
}

// document.open() / document.close() — RUM beacons (Nike's Boomerang/mPulse) do
// `O.document.open()` (writing a loader into an iframe's document, and our iframe
// contentWindow IS the realm) then `document.write(...)`. The binding had no `open`,
// so it threw "document.open is not a function" and aborted the beacon script. open()
// must return the document (so `d = document.open()` chains) and must NOT wipe the
// already-parsed DOM (a real open() clears, but wiping the hydrated tree is worse than
// a no-op here); close() is a no-op.
#[test]
fn document_open_close_do_not_throw_and_keep_dom() {
    let html = render_html(
        "<body><div id='keep'>x</div></body>",
        r#"
        const d = document.open();
        d.write("<p class='wrote'>hi</p>");
        document.close();
        document.getElementById('keep').setAttribute('data-open-ok', String(d === document));
        "#,
    )
    .unwrap();
    assert!(
        html.contains(r#"data-open-ok="true""#),
        "document.open() must return the document: {html}"
    );
    assert!(
        html.contains(r#"class="wrote""#),
        "document.write after open() must land: {html}"
    );
    assert!(
        html.contains(r#"id="keep""#),
        "document.open() must NOT wipe the already-parsed DOM: {html}"
    );
}

// performance.mark()/measure() must return the PerformanceEntry they create — Nike's
// Boomerang timing code destructures `const {startTime} = window.performance.mark(name)`
// (and reads `.duration`/`.entryType` off the measure), so a `mark()` returning undefined
// crashed with "Cannot destructure property 'startTime' of … as it is undefined".
#[test]
fn performance_mark_measure_return_entries() {
    let out = run_with_dom(
        "<body></body>",
        r#"
        const m = performance.mark("boot");
        const ms = performance.measure("span", "boot");
        JSON.stringify({
          sType: typeof m.startTime, dur: m.duration, et: m.entryType, nm: m.name,
          msStart: typeof ms.startTime, msDur: typeof ms.duration, msEt: ms.entryType, msNm: ms.name,
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sType"], "number", "mark().startTime is a number: {out}");
    assert_eq!(v["dur"], 0, "a mark has zero duration: {out}");
    assert_eq!(v["et"], "mark", "{out}");
    assert_eq!(v["nm"], "boot", "{out}");
    assert_eq!(v["msStart"], "number", "{out}");
    assert_eq!(v["msDur"], "number", "{out}");
    assert_eq!(v["msEt"], "measure", "{out}");
    assert_eq!(v["msNm"], "span", "{out}");
}

// React 18's scheduler drains its work queue via a MessageChannel (port2.postMessage
// → port1.onmessage). Without it, scheduled work (the hydration/mount the entry
// queues) never runs and nothing paints. MessageChannel must route a message to the
// other port's onmessage, drained by the timer/hydration pump.
#[test]
fn message_channel_drives_scheduled_work() {
    let html = render_html(
        "<body><div id='root'></div></body>",
        r#"
        const ch = new MessageChannel();
        ch.port1.onmessage = () => {
          const i = document.createElement('input');
          i.setAttribute('data-testid', 'scheduled');
          document.getElementById('root').appendChild(i);
        };
        ch.port2.postMessage(null); // schedule work — must reach port1.onmessage
        "#,
    )
    .unwrap();
    assert!(
        html.contains(r#"data-testid="scheduled""#),
        "MessageChannel must deliver to the other port's onmessage: {html}"
    );
}

// --- hydration (the headline tier-3 capability) -----------------------------
#[test]
fn page_script_hydrates_then_serializes() {
    let html = render_html(
        "<html><body><div id='app'></div></body></html>",
        r#"
        const app = document.getElementById('app');
        app.innerHTML = '<p class="msg">hydrated</p>';
        setTimeout(() => {
          const span = document.createElement('span');
          span.textContent = 'fromtimer';
          app.appendChild(span);
        }, 10);
        "#,
    )
    .unwrap();
    assert!(
        html.contains(r#"<p class="msg">hydrated</p>"#),
        "got: {html}"
    );
    assert!(html.contains("<span>fromtimer</span>"), "got: {html}");
}

#[tokio::test]
async fn mock_spa_hydrates_into_root() {
    // A framework-shaped bundle: mounts into #root, holds state, re-renders after an
    // effect (setTimeout) — the SPA hydration path end to end.
    let bundle = r#"
        const root = document.getElementById('root');
        let state = { count: 0, items: ['a', 'b'] };
        function render() {
          root.innerHTML = '';
          const app = document.createElement('div');
          app.setAttribute('class', 'app');
          const h = document.createElement('h1');
          h.textContent = 'Count: ' + state.count;
          app.appendChild(h);
          const ul = document.createElement('ul');
          for (const it of state.items) {
            const li = document.createElement('li');
            li.textContent = it;
            ul.appendChild(li);
          }
          app.appendChild(ul);
          root.appendChild(app);
        }
        render();                                   // initial mount
        setTimeout(() => {                          // effect → setState → re-render
          state.count = 5;
          state.items.push('c');
          render();
        }, 10);
    "#;
    let html = render_page(
        "<html><body><div id='root'></div></body></html>",
        "https://x.test/",
        bundle,
    )
    .await
    .unwrap()
    .replace("&nbsp;", " ");
    assert!(html.contains(r#"<div class="app">"#), "got: {html}");
    assert!(html.contains("<h1>Count: 5</h1>"), "got: {html}");
    assert!(html.contains("<li>c</li>"), "got: {html}");
}

#[tokio::test]
async fn async_promise_hydration_resolves() {
    let html = render_html_async(
        "<html><body><div id='app'></div></body></html>",
        r#"
        Promise.resolve().then(() => {
          document.getElementById('app').innerHTML = '<p>micro</p>';
        });
        (async () => {
          await new Promise((r) => setTimeout(r, 5));
          const s = document.createElement('span');
          s.textContent = 'awaited';
          document.getElementById('app').appendChild(s);
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(html.contains("<p>micro</p>"), "got: {html}");
    assert!(html.contains("<span>awaited</span>"), "got: {html}");
}

// --- DOM shims the render tier adds for real-world bundles (jQuery etc.) -----
#[test]
fn get_elements_by_tag_name_on_detached_subtree() {
    // jQuery's support probe builds a DETACHED div and reads
    // `div.getElementsByTagName('input')[0]`; the native query matches only
    // connected nodes, so the extension walks `children` instead.
    let out = run_with_dom(
        "<body></body>",
        r#"
        const div = document.createElement('div');
        div.innerHTML = '<input type="checkbox"><span>x</span><input>';
        JSON.stringify({
          inputs: div.getElementsByTagName('input').length,
          all: div.getElementsByTagName('*').length,
          last: div.lastChild ? String(div.lastChild.tagName) : null,
          lastEl: div.lastElementChild ? String(div.lastElementChild.tagName) : null,
        });
        "#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["inputs"], 2, "{out}");
    assert_eq!(v["all"], 3, "{out}");
    assert_eq!(v["last"], "INPUT", "{out}");
    assert_eq!(v["lastEl"], "INPUT", "{out}");
}

#[test]
fn document_write_appends_to_body() {
    let html = render_html(
        "<html><body></body></html>",
        r#"
        document.write("<div class='quote'>one</div>");
        document.write("<div class='quote'>two</div>");
        "#,
    )
    .unwrap();
    let n = html.matches(r#"class="quote""#).count();
    assert_eq!(n, 2, "got: {html}");
    assert!(
        html.contains(">one<") && html.contains(">two<"),
        "got: {html}"
    );
}

// The SPA differentiator: webpack's `__webpack_require__.e` injects a <script src>
// chunk at runtime and waits for its `onload` before mounting the app. A node DOM
// that only appends the <script> never runs it → the loader hangs → nothing paints.
// render_hydrate must FETCH + EXECUTE the injected chunk and fire onload, so the
// form (login-email-input) the onload builds actually renders.
#[tokio::test]
async fn dynamic_script_injection_runs_and_fires_onload() {
    let port = spawn_json_server("globalThis.__chunkRan = true;").await;
    let html = r#"<body><div id="root"></div>
      <script>
        const s = document.createElement('script');
        s.src = '/chunk.js';
        s.onload = () => {
          const i = document.createElement('input');
          i.setAttribute('data-testid', 'login-email-input');
          i.setAttribute('data-chunk', String(globalThis.__chunkRan === true));
          document.getElementById('root').appendChild(i);
        };
        document.head.appendChild(s);
      </script></body>"#;
    let out = render_hydrate(html, &base(port)).await.unwrap();
    assert!(
        out.contains(r#"data-testid="login-email-input""#),
        "injected chunk's onload must run and render the form: {out}"
    );
    assert!(
        out.contains(r#"data-chunk="true""#),
        "the injected chunk's own code must have executed: {out}"
    );
}

// Module-capable runtimes (us, every modern browser) SKIP `<script nomodule>` and
// run the module build instead. The hydration pump must honor this: a page's legacy
// polyfill bundle (e.g. Next's core-js `polyfill-nomodule`) overwrites native
// Promise/queueMicrotask with impls whose microtask scheduler is inert here, so
// running it makes the page's promises never settle and the render never converges.
// Both spellings appear in the wild: `nomodule` (HTML) and `noModule` (Next's
// server-rendered serialization of the React `noModule` prop).
#[tokio::test]
async fn nomodule_scripts_are_skipped() {
    for attr in ["nomodule", "noModule=\"\""] {
        let html = format!(
            r#"<body><div id="root"></div>
              <script {attr}>
                const bad = document.createElement('input');
                bad.setAttribute('data-testid', 'nomodule-ran');
                document.getElementById('root').appendChild(bad);
              </script>
              <script>
                const ok = document.createElement('input');
                ok.setAttribute('data-testid', 'module-ran');
                document.getElementById('root').appendChild(ok);
              </script></body>"#
        );
        let out = render_hydrate(&html, "https://example.test/")
            .await
            .unwrap();
        assert!(
            !out.contains(r#"data-testid="nomodule-ran""#),
            "[{attr}] nomodule script must NOT run: {out}"
        );
        assert!(
            out.contains(r#"data-testid="module-ran""#),
            "[{attr}] the non-nomodule script must still run: {out}"
        );
    }
}

// Dev-build support (B): an injected script that reads `import.meta.url` must NOT
// abort the page. We run every <script> as a CLASSIC script, where `import.meta` is a
// SyntaxError ("Cannot use 'import.meta' outside a module") — a turbopack `next dev`
// HMR runtime trips exactly this. The rewrite maps it onto the `__importMeta` global, so
// the read works AND a following script still runs.
#[tokio::test]
async fn import_meta_in_injected_script_does_not_abort_page() {
    let html = r#"<body><div id="root"></div>
      <script>
        const u = import.meta.url;
        const a = document.createElement('div');
        a.setAttribute('data-testid', 'import-meta');
        a.setAttribute('data-url', String(u));
        document.getElementById('root').appendChild(a);
      </script>
      <script>
        const b = document.createElement('div');
        b.setAttribute('data-testid', 'after-import-meta');
        document.getElementById('root').appendChild(b);
      </script></body>"#;
    let out = render_hydrate(html, "https://app.test/dashboard")
        .await
        .unwrap();
    assert!(
        out.contains(r#"data-testid="import-meta""#),
        "the import.meta script must run (not abort): {out}"
    );
    assert!(
        out.contains(r#"data-url="https://app.test/dashboard""#),
        "import.meta.url must read the page URL: {out}"
    );
    assert!(
        out.contains(r#"data-testid="after-import-meta""#),
        "the FOLLOWING script must still run after the import.meta one: {out}"
    );
}

// Dev-build support (B): a script using real `import`/`export` STATEMENTS can't run
// classically (no module loader headless) — it must be SKIPPED gracefully so the rest
// of the page still hydrates, not abort the whole pump.
#[tokio::test]
async fn esm_import_export_statements_skip_gracefully() {
    for esm in [
        "import x from '/y.js';",
        "export const v = 1;",
        "export {};",
    ] {
        let html = format!(
            r#"<body><div id="root"></div>
              <script>{esm}</script>
              <script>
                const ok = document.createElement('div');
                ok.setAttribute('data-testid', 'after-esm');
                document.getElementById('root').appendChild(ok);
              </script></body>"#
        );
        let out = render_hydrate(&html, "https://app.test/").await.unwrap();
        assert!(
            out.contains(r#"data-testid="after-esm""#),
            "[{esm}] a following script must still run after a skipped ESM script: {out}"
        );
    }
}

// Dev-build support (A): a page whose hydration never reaches idle (a dev-mode React
// loop) must return the BEST-EFFORT partial DOM rendered before the budget tripped,
// not an error — that partial render is what readable-error probes read.
#[tokio::test]
async fn hydrate_returns_partial_dom_on_budget_exceed() {
    // A first script renders real content, then a second spins forever. Pre-budget the
    // content is in the DOM; the spin trips the watchdog. We must get the content back.
    let html = r#"<body><div id="root"></div>
      <script>
        const m = document.createElement('div');
        m.setAttribute('data-testid', 'rendered-before-spin');
        document.getElementById('root').appendChild(m);
      </script>
      <script>while (true) {}</script></body>"#;
    let out = render_hydrate_with_budget(html, "https://app.test/", "", "", 300)
        .await
        .expect("budget-exceed must return best-effort DOM, not Err");
    assert!(
        out.contains(r#"data-testid="rendered-before-spin""#),
        "partial DOM rendered before the budget must be returned: {out}"
    );
}

// Dev-build support (A): a live session whose open() loops past the budget must still
// open (keeping the partial render + the live app), not fail.
#[tokio::test]
async fn live_session_opens_best_effort_on_budget_exceed() {
    let html = r#"<body><div id="root"></div>
      <script>
        const m = document.createElement('div');
        m.setAttribute('data-testid', 'open-partial');
        document.getElementById('root').appendChild(m);
      </script>
      <script>while (true) {}</script></body>"#;
    let session = PageSession::open(html, "https://app.test/", "", "", 300)
        .await
        .expect("open() must succeed best-effort on a budget-exceed");
    let out = session.serialize();
    session.close();
    assert!(
        out.contains(r#"data-testid="open-partial""#),
        "the live session must keep the partial render: {out}"
    );
}

// --- network: fetch / XHR over the tier-1 stack -----------------------------
#[tokio::test]
async fn mock_spa_fetches_data_via_xhr() {
    let port = spawn_json_server(r#"{"title":"From XHR"}"#).await;
    let bundle = r#"
        const xhr = new XMLHttpRequest();
        xhr.open('GET', '/data.json');
        xhr.onload = () => {
          const d = JSON.parse(xhr.responseText);
          document.getElementById('root').textContent = d.title;
        };
        xhr.send();
    "#;
    let html = render_page(
        "<body><div id='root'>loading</div></body>",
        &base(port),
        bundle,
    )
    .await
    .unwrap()
    .replace("&nbsp;", " ");
    assert!(html.contains("From XHR"), "got: {html}");
}

#[tokio::test]
async fn fetch_over_net_hydrates_from_localhost() {
    let port = spawn_json_server(r#"{"msg":"from-fetch"}"#).await;
    let html = render_page(
        "<html><body><div id='app'></div></body></html>",
        &base(port),
        r#"
        (async () => {
          const r = await fetch('/data.json');
          const j = await r.json();
          document.getElementById('app').textContent = j.msg + ':' + r.status;
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(html.contains("from-fetch:200"), "got: {html}");
}

#[tokio::test]
async fn fetch_failure_surfaces_as_failed_response() {
    // Nothing listening → fetch resolves to a failed Response (no throw).
    let html = render_page(
        "<html><body><div id='app'></div></body></html>",
        "http://127.0.0.1:1/",
        r#"
        (async () => {
          const r = await fetch('/x');
          document.getElementById('app').textContent = 'ok=' + r.ok + '/st=' + r.status;
        })();
        "#,
    )
    .await
    .unwrap();
    assert!(html.contains("ok=false/st=0"), "got: {html}");
}

// --- cookies + budget -------------------------------------------------------
#[tokio::test]
async fn document_cookie_bridge_roundtrips() {
    let html = render_page(
        "<html><body></body></html>",
        "https://x.test/",
        "document.cookie = 'a=1'; document.body.textContent = document.cookie;",
    )
    .await
    .unwrap();
    assert!(html.contains("a=1"), "got: {html}");
}

#[tokio::test]
async fn runaway_script_hits_render_budget() {
    let err = render_page_with_budget(
        "<body><div id='app'></div></body>",
        "https://x.test/",
        "while (true) {}",
        200,
    )
    .await
    .unwrap_err();
    assert!(err.contains("budget exceeded"), "got: {err}");
}

// A self-rescheduling `setTimeout` (analytics SDKs poll this way) must fire a BROWSER-LIKE,
// BOUNDED number of times — gated by the virtual clock — not spin to the raw count cap and
// starve the render. Without the virtual clock this loop fired ~100k times and the page
// never committed.
#[test]
fn virtual_clock_bounds_self_rescheduling_timers() {
    let out = render_html(
        "<body><div id='root'></div></body>",
        r#"
        let n = 0;
        (function poll() {
          n++;
          document.getElementById('root').setAttribute('data-n', String(n));
          setTimeout(poll, 100); // reschedules forever
        })();
        "#,
    )
    .unwrap();
    let n: usize = out
        .split("data-n=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // 15000ms virtual budget / 100ms ≈ 150 fires — bounded, NOT the old 100000 cap.
    assert!(
        n > 10 && n < 1000,
        "self-rescheduling poll bounded by virtual clock, got {n}: {out}"
    );
}

// `<iframe>.contentWindow` must expose the realm's builtins. Analytics SDKs (PostHog) read
// the native prototype of a builtin off a throwaway iframe's contentWindow; if it's missing
// they recreate an iframe on EVERY lookup (700+ churn that starves the render). With a real
// contentWindow the lookup succeeds + caches, so no churn.
#[test]
fn iframe_content_window_exposes_builtins() {
    let out = render_html(
        "<body><div id='root'></div></body>",
        r#"
        let ok = 0;
        for (let i = 0; i < 5; i++) {
          const f = document.createElement('iframe');
          document.body.appendChild(f);
          const w = f.contentWindow;
          if (w && w.Array && w.Array.prototype && w.Object && w.Object.prototype) ok++;
          document.body.removeChild(f);
        }
        document.getElementById('root').setAttribute('data-ok', String(ok));
        "#,
    )
    .unwrap();
    assert!(
        out.contains(r#"data-ok="5""#),
        "iframe.contentWindow exposes builtins: {out}"
    );
}

// Repro of the payroll wizard's partial hydration: a code-split bundle attaches its
// interactivity from a DYNAMICALLY injected <script> (webpack's
// `__webpack_require__.e`: appendChild a <script>, await its `onload`, then the chunk
// runs and wires handlers). If the render tier doesn't run a runtime-injected script
// AND fire its `load` so the awaiting promise resolves, the dependent code never runs
// and the element stays "un-hydrated" (no click handler) — exactly the dead
// "Add Manually" button. This must pass for the dynamic-import hydration path to work.
#[tokio::test]
async fn dynamically_injected_script_runs_and_fires_load() {
    let html = r#"<body>
      <div id="app">
        <button id="btn">go</button>
        <span data-test-id="status">cold</span>
      </div>
      <script>
        // The "chunk": when it runs it wires the delegated click handler and marks ready.
        function loadChunk() {
          return new Promise((resolve, reject) => {
            const s = document.createElement('script');
            s.textContent = "globalThis.__wireHandlers();";
            s.onload = () => resolve();
            s.onerror = () => reject(new Error('chunk failed'));
            document.head.appendChild(s);
          });
        }
        globalThis.__wireHandlers = () => {
          document.addEventListener('click', (e) => {
            if (e.target && e.target.id === 'btn') {
              document.querySelector('[data-test-id="status"]').textContent = 'hydrated';
            }
          });
        };
        // Like a lazy boundary: load the chunk, then mark the boundary resolved.
        loadChunk().then(() => {
          document.querySelector('[data-test-id="status"]').textContent = 'ready';
        });
      </script>
    </body>"#;

    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    // After hydration the boundary's promise must have resolved (chunk ran + onload fired).
    assert!(
        session
            .serialize()
            .contains(r#"data-test-id="status">ready<"#),
        "dynamic chunk must run and its load event resolve the awaiting promise: {}",
        session.serialize()
    );

    // And the handler the chunk wired must be live.
    session
        .eval(r#"document.getElementById('btn').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));"#)
        .await
        .unwrap();
    assert!(
        session
            .serialize()
            .contains(r#"data-test-id="status">hydrated<"#),
        "handler wired by the injected chunk must fire on click: {}",
        session.serialize()
    );

    session.close();
}

// PERMANENT headless-hydration harness (general rule: every hydration issue we fix
// gets a committable repro here). This fixture is REAL React 18 streaming SSR: a
// <Suspense> boundary that suspended on the server (streams its content late + a
// `$RC` completion script that walks the `<!--$?-->…<!--/$-->` comment markers and
// calls the dehydrated boundary's `_reactRetry`). React/ReactDOM UMD are inlined and
// `hydrateRoot` runs. The dehydrated boundary MUST hydrate in the render isolate —
// proven by the button's onClick firing (sets `window.__clicked`). Regenerate with
// tests/fixture-gen/gen-react-streaming.mjs. (Guards the comment-marker + `_reactRetry`
// hydration path; the Next App Router RSC-flight variant is tracked in HEADLESS-HYDRATION.md.)
#[tokio::test]
async fn react18_streaming_suspense_boundary_hydrates() {
    let html = include_str!("fixtures/react-streaming-hydration.html");
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    // The $RC completion swapped the streamed content in (button replaces fallback).
    let serialized = session.serialize();
    assert!(
        serialized.contains(r#"data-test-id="lazy-btn"#) && !serialized.contains(r#"id="fb""#),
        "streamed boundary content must replace the fallback: {serialized}"
    );

    // The dehydrated boundary must HYDRATE: clicking the button runs its React onClick.
    session
        .eval(r#"document.getElementById('btn').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })); globalThis.__RESULT = String(!!window.__clicked);"#)
        .await
        .unwrap();
    let clicked = session
        .eval(r#"globalThis.__RESULT = String(!!window.__clicked);"#)
        .await
        .unwrap();
    assert_eq!(
        clicked, "true",
        "the streamed/dehydrated Suspense boundary must hydrate (onClick must fire)"
    );

    session.close();
}

// Document-rooted hydration: Next.js App Router hydrates the WHOLE document
// (`ReactDOMClient.hydrateRoot(document, <App/>)`), not a div. This guards that a
// document-level root reaches hydrateRoot without throwing, React marks `document` as a
// root container, and the tree COMMITS + is interactive (the button's onClick fires).
// Fixture: tests/fixture-gen/gen-react-document.mjs (React/ReactDOM UMD inlined).
#[tokio::test]
async fn react_document_root_hydrates_and_commits() {
    let html = include_str!("fixtures/react-document-hydration.html");
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    let err = session
        .eval(r#"globalThis.__RESULT = String(window.__hydrateError || "");"#)
        .await
        .unwrap();
    assert_eq!(err, "", "hydrateRoot(document) must not throw");
    let called = session
        .eval(r#"globalThis.__RESULT = String(!!window.__hydrateCalled);"#)
        .await
        .unwrap();
    assert_eq!(called, "true", "hydrateRoot(document) must be reached");
    // Clicking the button must fire its React onClick — proves the document root COMMITTED
    // its fiber tree onto the SSR DOM and wired delegated listeners.
    session
        .eval(r#"document.querySelector('[data-test-id="doc-btn"]').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })); globalThis.__RESULT = "";"#)
        .await
        .unwrap();
    let clicked = session
        .eval(r#"globalThis.__RESULT = String(!!window.__clicked);"#)
        .await
        .unwrap();
    assert_eq!(
        clicked, "true",
        "document-rooted React app must hydrate + be interactive (onClick must fire)"
    );

    session.close();
}

// Client-side export capture: `URL.createObjectURL(blob)` → `<a download href=…>` →
// `link.click()` must be recorded in `__downloads` (filename + bytes) so the shim's
// page.waitForEvent('download') + download.path() work (CSV-template / file exports).
#[tokio::test]
async fn createobjecturl_anchor_download_is_captured() {
    let html = r#"<body><a id="dl">x</a>
      <script>
        var blob = new Blob(["a,b,c\n1,2,3"], { type: "text/csv" });
        var url = URL.createObjectURL(blob);
        var a = document.getElementById('dl');
        a.setAttribute('download', 'template.csv');
        a.setAttribute('href', url);
        a.click();
        var d = (globalThis.__downloads || [])[0] || {};
        document.body.setAttribute('data-fn', String(d.filename));
        document.body.setAttribute('data-content', String(d.content));
      </script></body>"#;
    let out = render_hydrate(html, "https://example.test/").await.unwrap();
    assert!(
        out.contains(r#"data-fn="template.csv""#),
        "download filename must be captured: {out}"
    );
    assert!(
        out.contains("a,b,c"),
        "download blob content must be captured: {out}"
    );
}

// __tcGetBy resolves getByRole/getByText/getByLabel IN the live isolate, returning each
// match's LIVE document-order index (querySelectorAll('*') position) so the shim dispatches
// on the SAME node it matched (a re-serialized snapshot can reorder portal'd nodes → wrong
// index). Guards role + accessible-name matching + that the returned idx maps to the right
// live element.
#[tokio::test]
async fn live_getby_returns_live_indices() {
    let html = r#"<body>
      <div><span>x</span><button>Cancel</button></div>
      <ul role="listbox"><li role="option">Alice</li><li role="option">Bob</li></ul>
    </body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    // role=option → two matches; verify each idx points at an <li role=option> in the live tree.
    let r = session
        .eval(r#"globalThis.__tcGetBy('role','option',null);
          var hits=JSON.parse(globalThis.__RESULT);
          var all=Array.prototype.slice.call(document.querySelectorAll('*'));
          globalThis.__RESULT = JSON.stringify(hits.map(function(h){ var e=all[h.idx]; return e.tagName+'/'+e.getAttribute('role')+'/'+e.textContent; }));"#)
        .await
        .unwrap();
    assert_eq!(
        r, r#"["LI/option/Alice","LI/option/Bob"]"#,
        "role=option must resolve to the two <li> by their live indices"
    );
    // role=button with accessible-name filter → the Cancel button only.
    let b = session
        .eval(r#"globalThis.__tcGetBy('role','button','Cancel');
          var hits=JSON.parse(globalThis.__RESULT);
          var all=Array.prototype.slice.call(document.querySelectorAll('*'));
          globalThis.__RESULT = JSON.stringify(hits.map(function(h){ return all[h.idx].tagName+':'+all[h.idx].textContent; }));"#)
        .await
        .unwrap();
    assert_eq!(
        b, r#"["BUTTON:Cancel"]"#,
        "role=button + name must resolve the Cancel button"
    );
    session.close();
}

// A click on a React PORTAL'd element (createPortal, here under createRoot(document.body),
// portal into body) must fire its onClick — the MUI Autocomplete/Dialog/Menu case (options
// render in a Popper portal to <body>; their onClick drives selection). React attaches
// delegated listeners per container (completeWork HostPortal → listenToAllSupportedEvents
// (containerInfo)); this guards that a click dispatched on a deep leaf of the portal'd <li>
// runs the <li>'s onClick (records its data-option-index). Fixture: gen-react-currenttarget.mjs.
#[tokio::test]
async fn portal_element_onclick_dispatches() {
    let html = include_str!("fixtures/react-currenttarget.html");
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    // Let the post-hydration effect mount the portal.
    for _ in 0..5 {
        session
            .eval(r#"if(globalThis.__runTimers)__runTimers(2000); globalThis.__RESULT = String(!!document.getElementById('leaf'));"#)
            .await
            .unwrap();
    }
    session
        .eval(r#"var l=document.getElementById('leaf'); if(l) l.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true})); globalThis.__RESULT="";"#)
        .await
        .unwrap();
    let ct = session
        .eval(r#"globalThis.__RESULT = String(window.__ct);"#)
        .await
        .unwrap();
    session.close();
    assert_eq!(
        ct, "2",
        "portal'd <li> onClick must fire (currentTarget's data-option-index)"
    );
}

// `document.location` must mirror `window.location` (a browser invariant). Next's dev RSC
// flight client reads `document.location.origin` (findSourceMapURL, replaying server
// console entries); when `document.location` was undefined it threw "reading 'origin' of
// undefined", which aborted the WHOLE flight stream → the App Router page silently never
// hydrated. Guards the fix at its root.
#[tokio::test]
async fn document_location_mirrors_window_location() {
    let html = r#"<body><div id="root"></div>
      <script>
        var r = document.getElementById('root');
        r.setAttribute('data-has', String(!!document.location));
        r.setAttribute('data-origin', String(document.location && document.location.origin));
        r.setAttribute('data-href', String(document.location && document.location.href));
      </script></body>"#;
    let out = render_hydrate(html, "https://app.test/some/path")
        .await
        .unwrap();
    assert!(
        out.contains(r#"data-has="true""#),
        "document.location must be defined: {out}"
    );
    assert!(
        out.contains(r#"data-origin="https://app.test""#),
        "document.location.origin must read the page origin: {out}"
    );
    assert!(
        out.contains(r#"data-href="https://app.test/some/path""#),
        "document.location.href must read the page URL: {out}"
    );
}

// The REAL react-server-dom-turbopack flight client (createFromReadableStream) must
// instantiate + parse a flight stream containing a client reference in our env, without
// throwing. Guards that the bundled flight client runs headless (process polyfill, stream
// reading, manifest resolution). Fixture: tests/fixture-gen/gen-flight-client.mjs.
#[tokio::test]
async fn flight_client_instantiates_and_parses() {
    let html = include_str!("fixtures/flight-client.html");
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    let started = session
        .eval(r#"globalThis.__RESULT = String(!!window.__flightStarted);"#)
        .await
        .unwrap();
    let flight_err = session
        .eval(r#"globalThis.__RESULT = String(window.__flightError || "");"#)
        .await
        .unwrap();
    session.close();
    assert_eq!(
        flight_err, "",
        "flight client must instantiate without throwing"
    );
    assert_eq!(
        started, "true",
        "createFromReadableStream must run + begin parsing the flight stream"
    );
}

// The exact App Router client-hydration SHAPE that wasn't committing in headless
// turbopack: hydrateRoot(document, …) called INSIDE React.startTransition, with a Suspense
// boundary whose child suspends on a thenable that resolves later (mimicking the RSC
// flight client awaiting a client-reference chunk). Must commit + be interactive once the
// thenable resolves. Fixture: tests/fixture-gen/gen-react-transition-suspense.mjs.
#[tokio::test]
async fn react_transition_document_suspense_commits() {
    let html = include_str!("fixtures/react-transition-suspense.html");
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");

    let err = session
        .eval(r#"globalThis.__RESULT = String(window.__hydrateError || "");"#)
        .await
        .unwrap();
    assert_eq!(
        err, "",
        "startTransition hydrateRoot(document) must not throw"
    );

    // The suspended child resolves (its thenable fires via the virtual timer); the tree
    // must COMMIT — clicking the (now-real) button fires its onClick.
    session
        .eval(r#"var b=document.querySelector('[data-test-id="tx-btn"]'); if(b){b.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true}));} globalThis.__RESULT = b ? "present" : "absent";"#)
        .await
        .unwrap();
    let clicked = session
        .eval(r#"globalThis.__RESULT = String(!!window.__clicked);"#)
        .await
        .unwrap();
    assert_eq!(
        clicked, "true",
        "transition+Suspense hydration on document must commit + be interactive after the thenable resolves"
    );

    session.close();
}

// Expando persistence: React stores its root + every fiber as an EXPANDO property on
// the DOM node (`container.__reactContainer$<rand>`, `node.__reactFiber$<rand>`). App
// Router hydrates the whole `document` (`hydrateRoot(document, …)`), so the container
// expando lands on the `document` object itself, and fibers land on documentElement/
// body/etc. If the DOM binding silently drops arbitrary JS properties on these special
// nodes, React "hydrates" but nothing is reachable → 0 fibers, no error. Guard that a
// plain JS property set+read round-trips on document, documentElement, head and body.
#[tokio::test]
async fn expando_properties_persist_on_document_and_root_nodes() {
    let html = r#"<html><head></head><body><div id="root"></div>
      <script>
        document.__tcProbe = 'doc';
        document.documentElement.__tcProbe = 'html';
        document.head.__tcProbe = 'head';
        document.body.__tcProbe = 'body';
        var r = document.getElementById('root');
        r.setAttribute('data-doc', String(document.__tcProbe));
        r.setAttribute('data-html', String(document.documentElement.__tcProbe));
        r.setAttribute('data-head', String(document.head.__tcProbe));
        r.setAttribute('data-body', String(document.body.__tcProbe));
      </script></body></html>"#;
    let out = render_hydrate(html, "https://example.test/").await.unwrap();
    for (attr, want) in [
        ("data-doc", "doc"),
        ("data-html", "html"),
        ("data-head", "head"),
        ("data-body", "body"),
    ] {
        assert!(
            out.contains(&format!(r#"{attr}="{want}""#)),
            "expando on {attr} node must persist (React root/fibers depend on it): {out}"
        );
    }
}

// Expandos on DYNAMICALLY-CREATED elements: React stores each fiber as `node.__reactFiber$
// <rand>` on the node it creates during render. For PORTAL'd content (MUI Dialog/Popper/
// Menu — createElement'd into a portal container, not hydrated from SSR), if a created
// node can't hold a JS expando, React commits the DOM but no fiber is reachable → event
// delegation can't find handlers → the portal renders but is DEAD (onClick never fires).
// Guard that a createElement'd + appended node round-trips an expando, before and after
// it's in the tree.
#[tokio::test]
async fn expando_properties_persist_on_created_elements() {
    let html = r#"<html><head></head><body><div id="root"></div>
      <script>
        var made = document.createElement('li');
        made.__tcFiber = 'before-append';
        var r = document.getElementById('root');
        r.setAttribute('data-before', String(made.__tcFiber));
        document.body.appendChild(made);
        made.__tcFiber2 = 'after-append';
        // re-fetch the same node from the live tree and read the expando back
        var again = document.body.lastElementChild;
        r.setAttribute('data-after', String(again.__tcFiber));
        r.setAttribute('data-after2', String(again.__tcFiber2));
      </script></body></html>"#;
    let out = render_hydrate(html, "https://example.test/").await.unwrap();
    for (attr, want) in [
        ("data-before", "before-append"),
        ("data-after", "before-append"),
        ("data-after2", "after-append"),
    ] {
        assert!(
            out.contains(&format!(r#"{attr}="{want}""#)),
            "expando on a created element must persist (React fibers on portal'd nodes depend on it): {out}"
        );
    }
}

// ESM support (foundation): a `<script type="module">` must EVALUATE, not be skipped.
// Next dev / turbopack serve their app as ES modules; the classic render tier used to
// skip any `import`/`export` script, so dev builds never hydrated. This guards the
// minimal module-eval path (no imports → no loader needed).
#[tokio::test]
async fn esm_inline_module_script_evaluates() {
    // A real ESM module (has `export`/`import` syntax) — the classic tier skipped these.
    let html = r#"<body><div id="app">x</div>
      <script type="module">export const tag = 'ok'; document.getElementById('app').setAttribute('data-esm', tag);</script>
    </body>"#;
    let out = render_hydrate(html, "https://example.test/").await.unwrap();
    assert!(
        out.contains(r#"data-esm="ok""#),
        "inline ESM module script must evaluate: {out}"
    );
}

// ESM loader: a `<script type="module">` that `import`s a sibling module must fetch +
// link it over the host net (the `NetModuleLoader`). This is the real dev-build path —
// the app entry module pulls its dependency graph by URL.
#[tokio::test]
async fn esm_module_import_graph_loads_over_net() {
    let port = spawn_js_server("export const v = 'imported-ok';").await;
    let html = r#"<body><div id="app">x</div>
      <script type="module">import { v } from '/mod.mjs'; document.getElementById('app').setAttribute('data-v', v);</script>
    </body>"#;
    let out = render_hydrate(html, &base(port)).await.unwrap();
    assert!(
        out.contains(r#"data-v="imported-ok""#),
        "ESM import graph must load over the net + evaluate: {out}"
    );
}

// ESM dev-build path: turbopack/webpack serve app chunks as CLASSIC `<script src>`
// (no `type=module`) whose BODY is an ES module (top-level import/export). The classic
// tier used to skip any import/export script, so those chunks never ran. Now a src chunk
// with an ESM body is handed to the module pump keyed by its src URL, so its import graph
// resolves relative to that URL. Guards item (1): src-ESM chunks hydrate.
#[tokio::test]
async fn esm_src_chunk_with_module_body_loads_and_resolves_imports() {
    // /chunk.mjs is an ESM that imports a sibling /dep.mjs and writes the result to DOM.
    let port = spawn_paths_js_server(&[
        (
            "/chunk.mjs",
            "import { v } from '/dep.mjs'; document.getElementById('app').setAttribute('data-chunk', v);",
        ),
        ("/dep.mjs", "export const v = 'chunk-ok';"),
    ])
    .await;
    // CLASSIC script element (no type=module) — exactly how dev chunks are served.
    let html = r#"<body><div id="app">x</div><script src="/chunk.mjs"></script></body>"#;
    let out = render_hydrate(html, &base(port)).await.unwrap();
    assert!(
        out.contains(r#"data-chunk="chunk-ok""#),
        "classic <script src> with an ESM body must route to the module pump + resolve its imports: {out}"
    );
}

// --- helpers ----------------------------------------------------------------
// Serves distinct JS bodies per request path (matched against the request line) —
// for ESM import graphs where the chunk and its dependency differ.
async fn spawn_paths_js_server(routes: &[(&'static str, &'static str)]) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let routes: Vec<(String, String)> = routes
        .iter()
        .map(|(p, b)| ((*p).to_string(), (*b).to_string()))
        .collect();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            // Buffer must hold the whole request: the in-V8 fetch now sends a
            // full Chrome header set (~700 B), and reading less leaves bytes
            // unread so the close-after-write RST-truncates the client's read.
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]).to_string();
            let path = req
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .split('#')
                .next()
                .unwrap_or("/")
                .to_string();
            let body = routes
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, b)| b.as_str())
                .unwrap_or("");
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.flush().await;
        }
    });
    port
}

// Serves any path with the given JS body (application/javascript) — for ESM imports.
async fn spawn_js_server(body: &'static str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let mut b = [0u8; 4096];
            let _ = s.read(&mut b).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.flush().await;
        }
    });
    port
}

async fn spawn_json_server(body: &'static str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let mut b = [0u8; 4096];
            let _ = s.read(&mut b).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.flush().await;
        }
    });
    port
}

fn base(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

// CSS :hover-revealed content (a hover dropdown / menu) must become visible when the shim
// hovers the trigger. turbo-dom's cascade does not apply the :hover pseudo-class (no pointer
// state), so a menu shown via `.trigger:hover .menu { display:block }` stays display:none and
// waitFor(visible) hangs. __tcApplyHover marks the hovered chain with [data-tc-hover], rewrites
// each stylesheet rule's `:hover` → `[data-tc-hover]`, and applies the matched rules' decls
// INLINE so both the JS getComputedStyle and rtdom's native cascade (is_visible) see the reveal.
// Real case: the app's UserMenu (overridden to open on hover) — the logout item lives in a
// `&:hover .user-menu` dropdown; the auth-logout e2e hovers the menu then clicks logout.
#[tokio::test]
async fn hover_reveals_css_hover_menu() {
    // Real shape: the dropdown is `visibility:hidden`, revealed by an emotion-style NESTED
    // `&:hover .dd { visibility:visible }` under the trigger's class (`&` = the trigger). The
    // flattener must resolve `&` to `.menu` and apply visibility inline on hover.
    let html = r#"<style>.dd{visibility:hidden}.menu{color:red;&:hover .dd{visibility:visible}}</style>
      <body><div class="menu" id="m"><span>trigger</span><div class="dd" id="d">logout</div></div></body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    // The `<style>` rule lives in rtdom's cascade (which is_visible reads); this env's
    // getComputedStyle doesn't parse <style> text, so assert on the inline style
    // __tcApplyHover applies — exactly what feeds the Rust cascade / is_visible.
    let before = session
        .eval(r#"globalThis.__RESULT = document.getElementById('d').style.visibility || '';"#)
        .await
        .unwrap();
    assert_eq!(before, "", "no inline visibility before hover");
    session
        .eval(r#"globalThis.__tcApplyHover(document.getElementById('m')); globalThis.__RESULT = 'ok';"#)
        .await
        .unwrap();
    let after = session
        .eval(r#"globalThis.__RESULT = document.getElementById('d').style.visibility || '';"#)
        .await
        .unwrap();
    assert_eq!(
        after, "visible",
        "hovering resolves the nested &:hover rule + applies visibility inline"
    );
    // is_visible reads the SERIALIZED snapshot, not the live DOM — the applied inline style
    // must survive serialization or the shim's waitFor(state:'visible') never observes it.
    let snap = session.serialize();
    assert!(
        snap.contains("visibility") && snap.contains("visible"),
        "serialized snapshot must carry the applied inline visibility: {snap}"
    );
    session.close();
}

// __tcGetBy(kind,value,name,root) scopes role/text/label matching to within elements matching
// `root` (descendant-or-self) — backs the shim's `parentLocator.getByRole/getByText/getByLabel`,
// so `step.getByTestId('x').getByRole('combobox')` drives the combobox INSIDE that step, not the
// first one in the document. idx stays the GLOBAL position so the shim dispatches on `*`[idx].
#[tokio::test]
async fn tcgetby_scopes_to_root() {
    let html = r#"<body>
      <div id="a"><div role="combobox">A</div></div>
      <div id="b"><div role="combobox">B</div></div>
    </body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    let unscoped = session
        .eval(
            r#"globalThis.__tcGetBy('role','combobox',null,null);
               globalThis.__RESULT = String(JSON.parse(globalThis.__RESULT).length);"#,
        )
        .await
        .unwrap();
    assert_eq!(unscoped, "2", "unscoped sees both comboboxes");
    let scoped = session
        .eval(
            r#"globalThis.__tcGetBy('role','combobox',null,'#a');
               var h = JSON.parse(globalThis.__RESULT);
               var all = Array.prototype.slice.call(document.querySelectorAll('*'));
               globalThis.__RESULT = h.length + ':' + (h[0] ? all[h[0].idx].textContent : '');"#,
        )
        .await
        .unwrap();
    assert_eq!(scoped, "1:A", "root='#a' scopes to the combobox inside #a");
    session.close();
}

// nth-scoped descendant resolution: `steps.nth(i).getByTestId('x').getByRole('combobox')` must
// drive the combobox inside the i-th step, not the first in the document. A CSS-concat selector
// can't express "the nth match's subtree", so the shim carries a scope CHAIN of {sel, idx} and
// __tcResolveScoped walks it (picking idx at each level) before matching the leaf (a selector OR
// a getBy). idx stays the GLOBAL position so the shim dispatches on `*`[idx]. Backs the
// payroll-approval-chain flow (two steps, each configured independently).
#[tokio::test]
async fn tcresolvescoped_walks_nth_chain() {
    let html = r#"<body>
      <div class="s"><div data-testid="x"><div role="combobox">A</div></div></div>
      <div class="s"><div data-testid="x"><div role="combobox">B</div></div></div>
    </body>"#;
    let mut session = PageSession::open(
        html,
        "https://example.test/",
        "",
        "",
        DEFAULT_RENDER_BUDGET_MS,
    )
    .await
    .expect("session opens");
    // getBy leaf scoped to the 2nd step → combobox "B"
    let b = session
        .eval(
            r#"globalThis.__tcResolveScoped(
                 [{"sel":".s","idx":1},{"sel":"[data-testid=\"x\"]","idx":null}],
                 {"getBy":{"kind":"role","value":"combobox","name":null}});
               var h = JSON.parse(globalThis.__RESULT);
               var all = Array.prototype.slice.call(document.querySelectorAll('*'));
               globalThis.__RESULT = h.length + ':' + (h[0] ? all[h[0].idx].textContent : '');"#,
        )
        .await
        .unwrap();
    assert_eq!(b, "1:B", "scope chain picks the 2nd step's combobox");
    // selector leaf scoped to the 1st step → its [data-testid=x]
    let a = session
        .eval(
            r#"globalThis.__tcResolveScoped([{"sel":".s","idx":0}], {"selector":"[data-testid=\"x\"]"});
               var h = JSON.parse(globalThis.__RESULT);
               var all = Array.prototype.slice.call(document.querySelectorAll('*'));
               globalThis.__RESULT = h.length + ':' + (h[0] ? all[h[0].idx].querySelector('[role=combobox]').textContent : '');"#,
        )
        .await
        .unwrap();
    assert_eq!(a, "1:A", "selector leaf scoped to the 1st step");
    session.close();
}
