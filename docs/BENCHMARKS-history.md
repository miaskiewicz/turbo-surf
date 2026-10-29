# Benchmark history

turbo-surf 0.6.0 (the turbo-dom consolidation: DOM, layout, raster and the browser
environment now come from the shared turbo-dom crates, and the render tier moved to the
stock V8 15.0 engine) has **not been re-benchmarked yet**. A re-run of all four turbo-*
repos together, on a quiet machine, is planned after the release. The numbers below are
the last measured ones, kept for history only: they describe the engine **before** the
consolidation and are not 0.6.0 numbers.

Provenance: the tables were last edited in the README between 2026-06-18
(`b372c29`, token-bucket politeness + external-script cache; head-to-head bench) and the
0.5.x releases; they are live-network, machine-dependent runs.

## Pre-consolidation: competitive benchmark

`harness/competitive/` runs the **same Playwright script** on turbo-surf and a
fleet of real browsers, scoring output **parity** against a Chromium oracle and
timing each. `npm run harness`. turbo-surf drives every routine through its native
engine — turbo-dom + a `deno_core` V8 render tier for page JS — with **no Chromium**.
Median ms over 8 runs (live network), parity is each engine's observations vs the
Chromium oracle:

| engine | wikipedia | js-quotes | parity |
|---|---|---|---|
| **turbo-surf (no-JS)** | **142** | — *(needs JS)* | ✓ |
| **turbo-surf (JS)** | —‡ | **132** | ✓ |
| chromium *(oracle)* | 932 | 933 | — |
| firefox | 727 | 925 | ✓ |
| webkit | 1232 | 964 | ✓ |

Every engine produces the **same observations** as Chromium / Firefox / WebKit
(parity ✓) — and turbo-surf is the **fastest in the table** on both axes. The
Wikipedia click-through runs in **142 ms** (**~6.6× faster than Chromium**, 932),
and the **real jQuery on `quotes.toscrape.com/js`** — the same 10 quotes Chromium
extracts — in **132 ms** (**~7× faster than Chromium**, 933), inside a true V8
isolate over a native rtdom DOM, **no Chromium process**. It stays network-bound
via a **pooled HTTP client** (connection/TLS reuse across pages), a **persistent V8
isolate** whose **DOM install is reused across same-page `page.evaluate`s** (parse
once per page, ~0.5 ms/call after; a different page gets a fresh realm), an **external-script cache** (jQuery fetched
once, not per page), a **per-page parse cache**, and a back-forward snapshot cache
so `goBack` restores instead of re-fetching. Profilers: `cargo bench -p
turbo-surf-view` (Rust microbench) + `harness/hotpath/rust-hotpath.mjs`.

‡ JS mode runs the *page's own* scripts; on a server-rendered page like Wikipedia
that over-runs (use no-JS there — 142 ms, 4/4). The `form` routine is omitted this
run (httpbin.org was returning 503/timeouts for every engine).

The harness auto-detects installed engines (`firefox`/`webkit`, and anti-detect
browsers like `playwright-extra`/`patchright`/`rebrowser-playwright`); see
[harness/competitive/README.md](../harness/competitive/README.md). (Numbers are
network-bound and machine/run dependent.)

## Pre-consolidation: crawler-vs-crawler benchmark

`harness/crawlers/` races turbo-surf against other open-source crawlers on a
real, paginated site — **same** 20-page same-host crawl of `books.toscrape.com`,
**same** ~150 ms per-request politeness on every engine, items counted with the
**same** CSS selector. Median of 3 timed runs, live network (`npm run
crawl-bench`):

| crawler | runtime model | items | median ms | pages/s |
|---|---|---|---|---|
| crawlee `CheerioCrawler` | Node | 339 | 2767 | 7.2 |
| **turbo-surf (no-js)** | **native Rust, browserless** | 339 | 3271 | **6.1** |
| spider-rs | Rust + N-API | 194 | 3486 | 5.7 |
| got + cheerio (hand-rolled) | Node | 339 | 5590 | 3.6 |
| node-crawler (`crawler`) | Node | 339 | 49624 | 0.4 |
| Scrapy | Python *(subprocess)* | 246 | 49270 | 0.4 |
| Colly | Go *(subprocess)* | 320 | 45664 | 0.4 |

**Head-to-head vs CheerioCrawler** (the closest competitor) — the **same** 20-page
crawl, `maxConcurrency = 2`, median of 5 warm runs
(`node harness/crawlers/head-to-head.mjs`). With throttling **off** (raw engine
speed — the truest apples-to-apples, since the two *throttle models* differ)
turbo-surf is clearly ahead:

| engine | politeness | median ms | pages/s |
|---|---|---|---|
| **turbo-surf (no-js)** | none (raw) | **1977** | **10.1** |
| crawlee CheerioCrawler | none (raw) | 2748 | 7.3 |
| crawlee CheerioCrawler | 150 ms rate | 2634 | 7.6 |
| **turbo-surf (no-js)** | 150 ms (strict) | 3307 | 6.0 |

Raw, **turbo-surf is ~1.4× faster**. Under the "150 ms politeness" rows it looks
slower only because turbo-surf's per-host gate is a **strict** interval (it really
waits 150 ms between requests), whereas crawlee's `maxRequestsPerMinute` is a
lenient sliding window that lets a short 20-page burst through near-raw. Same
content (339 items), no browser.

At equal politeness the multi-engine wall-clock is **network-bound**, so the
in-process crawlers cluster together: turbo-surf sits in the **top tier** alongside
the dedicated speed engines (crawlee, spider-rs) and ahead of a hand-rolled
got+cheerio loop — while extracting equivalent content and running **no
browser**. turbo-surf runs the whole BFS — fetch, parse, same-host gate, per-page
item count — in its native Rust engine, and is **~13× ahead of Scrapy / Colly**.
The heavyweight
engines trail ~15×: Scrapy and Colly pay a fresh
process startup per crawl (the harness shells out to their CLIs), and
node-crawler's per-request overhead is high. turbo-dom's raw parse advantage
doesn't show here — at 20 live pages, network swamps a sub-millisecond parse;
it shows in-memory instead (links ~18k/s, crawl ~14k pages/s, below). And unlike
every engine in this table, the *same* turbo-surf also runs Playwright scripts
(parity table above).

### JS-executing crawlers — turbo-surf vs real browsers

The other set targets `quotes.toscrape.com/js`, where quotes are built
client-side (a non-JS crawler sees ~0). turbo-surf runs the page's own scripts in
a true V8 isolate over its native DOM, while every competitor drives a **real
headless Chromium** (`npm run crawl-bench:js`, 11 pages, median of 3):

| crawler | JS engine | items | median ms | pages/s |
|---|---|---|---|---|
| **turbo-surf (JS)** | **V8 isolate, no browser** | 110 | **3031** | **3.63** |
| crawlee `PuppeteerCrawler` | headless Chromium | 110 | 4330 | 2.54 |
| crawlee `PlaywrightCrawler` | headless Chromium | 110 | 5569 | 1.98 |
| puppeteer-cluster | headless Chromium | 110 | 20024 | 0.55 |

turbo-surf runs each page's own scripts in a **true V8 isolate** over the native
rtdom DOM (the same path that renders `quotes.toscrape.com/js`, external scripts
cached across pages) — the **fastest engine here**, extracting the same 110 quotes
a real browser does **~1.4–6.6× faster than the browser-driving crawlers**, with no
Chromium process and honoring the 150 ms politeness delay. Running page JS in a
true isolate against a lightweight DOM is rare — most crawlers either drive a full
browser (this table) or use an in-process fake DOM with no real isolation. See
[harness/crawlers/README.md](../harness/crawlers/README.md) — competitors
auto-detect and missing ones are skipped; install them with
`npm i -D @spider-rs/spider-rs crawlee cheerio got crawler` (+ `brew install
pipx go && pipx install scrapy` for Scrapy/Colly). (Machine/run dependent.)
