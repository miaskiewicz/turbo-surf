//! Crawler scheduling (port of the pure logic in `src/crawl.mjs`, SPEC §9):
//! frontier drive with global + per-host concurrency, per-host politeness, retry
//! with exponential backoff, and depth/page caps. The actual page fetch+parse is
//! a tier-2 concern, abstracted here behind the `Navigator` trait; the tier-2
//! `Page` (built on the turbo-dom Rust crate) will implement it. robots.txt
//! integration rides in with that wiring — `robots.rs` is already done.

use crate::frontier::{Frontier, Item};
use crate::robots::{RobotsCache, RobotsFetcher};
use crate::url::{host_of, is_http_url, origin_of};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;

/// One navigation result. The tier-2 `Page` produces this; tests stub it.
#[derive(Clone, Debug, Default)]
pub struct Nav {
    pub url: String,
    pub status: u16,
    pub title: String,
    pub links: Vec<String>,
    pub error: Option<String>,
    /// Count of elements matching the crawl's item selector on this page (0 when no
    /// selector is configured). Lets the crawler report extraction counts without a
    /// second pass — used by the crawler-vs-crawler benchmark for correctness parity.
    pub items: u32,
}

/// The fetch+parse seam. `goto` resolves a URL to a `Nav`, or `Err` for a
/// transport-level failure (retryable per the backoff budget).
#[async_trait]
pub trait Navigator: Send + Sync {
    async fn goto(&self, url: &str) -> Result<Nav, String>;
}

/// robots.txt gate for the crawl driver (object-safe so `CrawlOptions` stays
/// non-generic). [`SharedRobots`] adapts a [`RobotsCache`].
#[async_trait]
pub trait RobotsGate: Send + Sync {
    async fn allowed(&self, url: &str, ua: &str, now: u64) -> bool;
    async fn crawl_delay(&self, origin: &str, ua: &str, now: u64) -> Option<f64>;
}

/// Wraps a [`RobotsCache`] in an async mutex so the shared crawl driver can gate
/// concurrently (the cache mutates on first fetch per origin).
pub struct SharedRobots<F: RobotsFetcher>(AsyncMutex<RobotsCache<F>>);

impl<F: RobotsFetcher> SharedRobots<F> {
    pub fn new(cache: RobotsCache<F>) -> Self {
        Self(AsyncMutex::new(cache))
    }
}

#[async_trait]
impl<F: RobotsFetcher + Send + Sync> RobotsGate for SharedRobots<F> {
    async fn allowed(&self, url: &str, ua: &str, now: u64) -> bool {
        self.0.lock().await.allowed(url, ua, now).await
    }
    async fn crawl_delay(&self, origin: &str, ua: &str, now: u64) -> Option<f64> {
        self.0.lock().await.crawl_delay(origin, ua, now).await
    }
}

/// Output record, mirroring the JS crawl record shape (sans the lazy views,
/// which are tier-2).
#[derive(Clone, Debug)]
pub struct Record {
    pub url: String,
    pub status: u16,
    pub depth: usize,
    pub title: String,
    pub links: Vec<String>,
    pub error: Option<String>,
    pub items: u32,
}

type AllowFn = Box<dyn Fn(&str) -> bool + Send + Sync>;

pub struct CrawlOptions {
    pub start: Vec<String>,
    pub max_pages: usize,
    pub max_depth: usize,
    pub concurrency: usize,
    pub per_host_concurrency: usize,
    pub politeness_ms: u64,
    pub same_host_only: bool,
    pub retry_budget: u32,
    pub backoff_ms: u64,
    pub allow: Option<AllowFn>,
    pub user_agent: String,
    /// robots.txt gate (allow + crawl-delay). `None` = no robots checks.
    pub robots: Option<Arc<dyn RobotsGate>>,
}

impl Default for CrawlOptions {
    fn default() -> Self {
        Self {
            start: Vec::new(),
            max_pages: 100,
            max_depth: 3,
            concurrency: 4,
            per_host_concurrency: 2,
            politeness_ms: 0,
            same_host_only: true,
            retry_budget: 2,
            backoff_ms: 200,
            allow: None,
            user_agent: "turbo-surf".to_string(),
            robots: None,
        }
    }
}

fn retryable(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

fn should_retry_status(o: &CrawlOptions, status: u16, attempt: u32) -> bool {
    retryable(status) && attempt < o.retry_budget
}

// --- link acceptance (pure) -------------------------------------------------

fn host_blocked(o: &CrawlOptions, start_hosts: &HashMap<String, ()>, url: &str) -> bool {
    if !o.same_host_only {
        return false;
    }
    match host_of(url) {
        Some(h) => !start_hosts.contains_key(&h),
        None => true,
    }
}

fn allow_blocked(o: &CrawlOptions, url: &str) -> bool {
    match &o.allow {
        Some(f) => !f(url),
        None => false,
    }
}

fn accept_link(o: &CrawlOptions, start_hosts: &HashMap<String, ()>, url: &str) -> bool {
    is_http_url(url) && !host_blocked(o, start_hosts, url) && !allow_blocked(o, url)
}

// --- per-host scheduling state ----------------------------------------------

#[derive(Default, Clone)]
struct HostState {
    in_flight: usize,
    next_at: u64,
    politeness_ms: Option<u64>,
}

struct Shared {
    frontier: Frontier,
    host_state: HashMap<String, HostState>,
    start_hosts: HashMap<String, ()>,
    produced: usize,
    active: usize,
}

impl Shared {
    fn host_mut(&mut self, host: &str) -> &mut HostState {
        self.host_state.entry(host.to_string()).or_default()
    }

    fn is_drained(&self) -> bool {
        self.active == 0 && self.frontier.pending() == 0
    }
}

// Whether an item's host is under its concurrency cap and past its politeness
// gate; tracks the minimum wait across deferred items in `min_wait`.
//
// Politeness is a per-host token bucket of rate 1 / `politeness_ms` with burst =
// `per_host_concurrency` — i.e. `cap` requests may overlap, then dispatches space
// out at the politeness interval (this is what crawlee's
// `maxRequestsPerMinute` + `maxConcurrency` does). `next_at` accumulates the
// interval per dispatch; a request is ready while `next_at` is no more than
// `(cap-1)` intervals ahead of now. (`cap == 1` → the strict serial gap.)
fn item_ready(
    sh: &mut Shared,
    o: &CrawlOptions,
    item: &Item,
    now: u64,
    min_wait: &mut u64,
) -> bool {
    let cap = o.per_host_concurrency;
    let host = host_of(&item.url).unwrap_or_default();
    let burst = o.politeness_ms.saturating_mul(cap.saturating_sub(1) as u64);
    let st = sh.host_mut(&host);
    if st.in_flight >= cap {
        return false;
    }
    if st.next_at > now + burst {
        *min_wait = (*min_wait).min(st.next_at - burst - now);
        return false;
    }
    true
}

struct Claim {
    item: Option<Item>,
    wait: u64,
}

// Pull the next fetchable item whose host is under its cap and past its
// politeness gate; re-queue everything passed over.
fn claim(sh: &mut Shared, o: &CrawlOptions, now: u64) -> Claim {
    let mut deferred = Vec::new();
    let mut min_wait = u64::MAX;
    let mut ready = None;
    while let Some(item) = sh.frontier.next() {
        if item_ready(sh, o, &item, now, &mut min_wait) {
            ready = Some(item);
            break;
        }
        deferred.push(item);
    }
    for d in deferred {
        sh.frontier.requeue(d);
    }
    if ready.is_some() {
        return Claim {
            item: ready,
            wait: 0,
        };
    }
    Claim {
        item: None,
        wait: if min_wait == u64::MAX { 0 } else { min_wait },
    }
}

fn enqueue_links(sh: &mut Shared, o: &CrawlOptions, depth: usize, urls: &[String]) {
    for url in urls {
        if accept_link(o, &sh.start_hosts, url) {
            sh.frontier.add(url, depth + 1);
        }
    }
}

// --- driver -----------------------------------------------------------------

fn seed(sh: &mut Shared, o: &CrawlOptions) {
    for s in o.start.iter().filter(|s| is_http_url(s)) {
        if let Some(h) = host_of(s) {
            sh.start_hosts.insert(h, ());
        }
        sh.frontier.add(s, 0);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// Resolve a host's effective politeness once (cached), then stamp next_at.
fn record_politeness(sh: &mut Shared, o: &CrawlOptions, host: &str) {
    let now = now_ms();
    let st = sh.host_mut(host);
    let ms = st.politeness_ms.get_or_insert(o.politeness_ms).to_owned();
    // Accumulate the interval (token bucket) so `burst` overlapping dispatches are
    // allowed, then subsequent ones space out — instead of resetting to `now`,
    // which would hard-serialize every fetch one interval apart.
    st.next_at = st.next_at.max(now) + ms;
}

struct Ctx {
    shared: Arc<Mutex<Shared>>,
    opts: Arc<CrawlOptions>,
    nav: Arc<dyn Navigator>,
    tx: mpsc::UnboundedSender<Record>,
}

async fn backoff(o: &CrawlOptions, attempt: u32) -> u32 {
    let next = attempt + 1;
    let ms = o.backoff_ms.saturating_mul(1u64 << (next - 1));
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    next
}

fn error_record(item: &Item, err: String) -> Record {
    Record {
        url: item.url.clone(),
        status: 0,
        depth: item.depth,
        title: String::new(),
        links: Vec::new(),
        error: Some(err),
        items: 0,
    }
}

// Navigate once with retry/backoff on retryable statuses or thrown errors.
async fn goto_with_retry(ctx: &Ctx, item: &Item, host: &str) -> Result<Nav, Record> {
    let mut attempt = 0;
    loop {
        let result = ctx.nav.goto(&item.url).await;
        record_politeness(&mut ctx.shared.lock().unwrap(), &ctx.opts, host);
        match result {
            Ok(nav) if should_retry_status(&ctx.opts, nav.status, attempt) => {
                attempt = backoff(&ctx.opts, attempt).await;
            }
            Ok(nav) => return Ok(nav),
            Err(_) if attempt < ctx.opts.retry_budget => {
                attempt = backoff(&ctx.opts, attempt).await;
            }
            Err(e) => return Err(error_record(item, e)),
        }
    }
}

fn record_of(item: &Item, nav: &Nav) -> Record {
    Record {
        url: if nav.url.is_empty() {
            item.url.clone()
        } else {
            nav.url.clone()
        },
        status: nav.status,
        depth: item.depth,
        title: nav.title.clone(),
        links: nav.links.clone(),
        error: nav.error.clone(),
        items: nav.items,
    }
}

// Publish a record + harvest links under the lock; returns true to stop (cap hit).
fn publish(ctx: &Ctx, item: &Item, nav: &Nav) -> bool {
    let mut sh = ctx.shared.lock().unwrap();
    if sh.produced >= ctx.opts.max_pages {
        return true;
    }
    sh.produced += 1;
    let _ = ctx.tx.send(record_of(item, nav));
    if item.depth < ctx.opts.max_depth {
        enqueue_links(&mut sh, &ctx.opts, item.depth, &nav.links);
    }
    false
}

// robots.txt allow gate (true when no robots configured).
async fn robots_allows(ctx: &Ctx, url: &str) -> bool {
    match &ctx.opts.robots {
        Some(r) => r.allowed(url, &ctx.opts.user_agent, now_ms()).await,
        None => true,
    }
}

// Resolve a host's politeness once: max of the configured delay and any robots
// Crawl-delay (seconds → ms). Cached on the host state (robots consulted once).
async fn resolve_politeness(ctx: &Ctx, host: &str, url: &str) {
    let already = ctx
        .shared
        .lock()
        .unwrap()
        .host_state
        .get(host)
        .is_some_and(|s| s.politeness_ms.is_some());
    if already {
        return;
    }
    let mut ms = ctx.opts.politeness_ms;
    if let (Some(r), Some(origin)) = (&ctx.opts.robots, origin_of(url)) {
        if let Some(cd) = r.crawl_delay(&origin, &ctx.opts.user_agent, now_ms()).await {
            ms = ms.max((cd * 1000.0) as u64);
        }
    }
    ctx.shared
        .lock()
        .unwrap()
        .host_mut(host)
        .politeness_ms
        .get_or_insert(ms);
}

async fn process_item(ctx: &Ctx, item: &Item) -> bool {
    let host = host_of(&item.url).unwrap_or_default();
    if !robots_allows(ctx, &item.url).await {
        return false; // disallowed by robots → skip, no record
    }
    {
        let mut sh = ctx.shared.lock().unwrap();
        sh.host_mut(&host).in_flight += 1;
        sh.active += 1;
    }
    resolve_politeness(ctx, &host, &item.url).await;
    let stop = match goto_with_retry(ctx, item, &host).await {
        Ok(nav) => publish(ctx, item, &nav),
        Err(rec) => {
            let _ = ctx.tx.send(rec);
            false
        }
    };
    let mut sh = ctx.shared.lock().unwrap();
    sh.host_mut(&host).in_flight -= 1;
    sh.active -= 1;
    stop
}

fn try_claim(ctx: &Ctx) -> Claim {
    let mut sh = ctx.shared.lock().unwrap();
    claim(&mut sh, &ctx.opts, now_ms())
}

fn reached_cap(ctx: &Ctx) -> bool {
    let sh = ctx.shared.lock().unwrap();
    sh.produced >= ctx.opts.max_pages
}

fn drained(ctx: &Ctx) -> bool {
    ctx.shared.lock().unwrap().is_drained()
}

async fn worker(ctx: Arc<Ctx>) {
    while !reached_cap(&ctx) {
        let Claim { item, wait } = try_claim(&ctx);
        match item {
            Some(item) => {
                if process_item(&ctx, &item).await {
                    return;
                }
            }
            None => {
                if drained(&ctx) {
                    return;
                }
                let ms = if wait > 0 { wait } else { 5 };
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
        }
    }
}

/// Run a crawl to completion, returning all records. Concurrency, per-host caps,
/// politeness, retry/backoff, and depth/page limits all honored.
pub async fn crawl(opts: CrawlOptions, nav: Arc<dyn Navigator>) -> Vec<Record> {
    let mut shared = Shared {
        frontier: Frontier::new(),
        host_state: HashMap::new(),
        start_hosts: HashMap::new(),
        produced: 0,
        active: 0,
    };
    seed(&mut shared, &opts);

    let n = opts.concurrency.max(1);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let ctx = Arc::new(Ctx {
        shared: Arc::new(Mutex::new(shared)),
        opts: Arc::new(opts),
        nav,
        tx,
    });

    let workers: Vec<_> = (0..n).map(|_| tokio::spawn(worker(ctx.clone()))).collect();
    drop(ctx); // drop our tx clone so rx closes when workers finish

    let mut out = Vec::new();
    while let Some(rec) = rx.recv().await {
        out.push(rec);
    }
    for w in workers {
        let _ = w.await;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(start: &[&str]) -> CrawlOptions {
        CrawlOptions {
            start: start.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn retryable_statuses() {
        assert!(retryable(429));
        assert!(retryable(503));
        assert!(!retryable(404));
        assert!(!retryable(200));
    }

    #[test]
    fn accept_link_same_host_and_scheme() {
        let mut o = opts(&["https://x.test/"]);
        let mut hosts = HashMap::new();
        hosts.insert("x.test".to_string(), ());
        assert!(accept_link(&o, &hosts, "https://x.test/a"));
        assert!(!accept_link(&o, &hosts, "https://other.test/a"));
        assert!(!accept_link(&o, &hosts, "mailto:a@b.test"));
        o.same_host_only = false;
        assert!(accept_link(&o, &hosts, "https://other.test/a"));
    }

    #[test]
    fn allow_predicate_blocks() {
        let mut o = opts(&["https://x.test/"]);
        o.allow = Some(Box::new(|u: &str| u.contains("/keep")));
        let mut hosts = HashMap::new();
        hosts.insert("x.test".to_string(), ());
        assert!(accept_link(&o, &hosts, "https://x.test/keep/1"));
        assert!(!accept_link(&o, &hosts, "https://x.test/drop/1"));
    }

    struct Site {
        pages: HashMap<String, Nav>,
    }
    #[async_trait]
    impl Navigator for Site {
        async fn goto(&self, url: &str) -> Result<Nav, String> {
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| format!("404 {url}"))
        }
    }

    fn nav(url: &str, links: &[&str]) -> Nav {
        Nav {
            url: url.to_string(),
            status: 200,
            title: "t".to_string(),
            links: links.iter().map(|s| s.to_string()).collect(),
            error: None,
            items: 0,
        }
    }

    #[tokio::test]
    async fn crawls_and_follows_links_same_host() {
        let mut pages = HashMap::new();
        pages.insert(
            "https://x.test/".to_string(),
            nav(
                "https://x.test/",
                &["https://x.test/a", "https://other.test/z"],
            ),
        );
        pages.insert("https://x.test/a".to_string(), nav("https://x.test/a", &[]));
        let site = Arc::new(Site { pages });

        let mut recs = crawl(opts(&["https://x.test/"]), site).await;
        recs.sort_by(|a, b| a.url.cmp(&b.url));
        let urls: Vec<_> = recs.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x.test/", "https://x.test/a"]);
    }

    #[tokio::test]
    async fn respects_max_pages() {
        let mut pages = HashMap::new();
        let links: Vec<String> = (0..10).map(|i| format!("https://x.test/p{i}")).collect();
        let link_refs: Vec<&str> = links.iter().map(String::as_str).collect();
        pages.insert(
            "https://x.test/".to_string(),
            nav("https://x.test/", &link_refs),
        );
        for l in &links {
            pages.insert(l.clone(), nav(l, &[]));
        }
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            max_pages: 3,
            ..opts(&["https://x.test/"])
        };
        let recs = crawl(o, site).await;
        assert_eq!(recs.len(), 3);
    }

    #[tokio::test]
    async fn respects_max_depth() {
        let mut pages = HashMap::new();
        pages.insert(
            "https://x.test/".to_string(),
            nav("https://x.test/", &["https://x.test/a"]),
        );
        pages.insert(
            "https://x.test/a".to_string(),
            nav("https://x.test/a", &["https://x.test/b"]),
        );
        pages.insert("https://x.test/b".to_string(), nav("https://x.test/b", &[]));
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            max_depth: 1,
            ..opts(&["https://x.test/"])
        };
        let mut recs = crawl(o, site).await;
        recs.sort_by(|a, b| a.url.cmp(&b.url));
        let urls: Vec<_> = recs.iter().map(|r| r.url.as_str()).collect();
        // depth 0 root + depth 1 /a; /b (depth 2) never enqueued
        assert_eq!(urls, vec!["https://x.test/", "https://x.test/a"]);
    }

    #[tokio::test]
    async fn error_pages_recorded() {
        let pages = HashMap::new(); // start 404s
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            backoff_ms: 0, // keep the retry loop fast
            ..opts(&["https://x.test/"])
        };
        let recs = crawl(o, site).await;
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, 0);
        assert!(recs[0].error.is_some());
    }

    fn empty_shared() -> Shared {
        let mut sh = Shared {
            frontier: Frontier::new(),
            host_state: HashMap::new(),
            start_hosts: HashMap::new(),
            produced: 0,
            active: 0,
        };
        sh.start_hosts.insert("x.test".to_string(), ());
        sh
    }

    #[test]
    fn host_blocked_true_when_host_unparseable() {
        let o = opts(&["https://x.test/"]);
        let hosts: HashMap<String, ()> = HashMap::new();
        // is_http true but host_of None (no authority) → blocked.
        assert!(host_blocked(&o, &hosts, "mailto:a@b.test"));
    }

    #[test]
    fn claim_defers_at_cap_and_reports_politeness_wait() {
        let o = CrawlOptions {
            per_host_concurrency: 1,
            ..opts(&["https://x.test/"])
        };
        let mut sh = empty_shared();
        sh.frontier.add("https://x.test/a", 0);
        // Host at its concurrency cap → item deferred (requeued), nothing ready.
        sh.host_mut("x.test").in_flight = 1;
        let c = claim(&mut sh, &o, 0);
        assert!(c.item.is_none());
        assert_eq!(c.wait, 0); // no politeness wait pending → clamps to 0
        assert_eq!(sh.frontier.pending(), 1); // requeued

        // Now under cap but inside the politeness window → wait reported.
        sh.host_mut("x.test").in_flight = 0;
        sh.host_mut("x.test").next_at = 100;
        let c2 = claim(&mut sh, &o, 10);
        assert!(c2.item.is_none());
        assert_eq!(c2.wait, 90);

        // Past the window → item is claimed.
        let c3 = claim(&mut sh, &o, 200);
        assert!(c3.item.is_some());
    }

    #[test]
    fn record_of_falls_back_to_item_url_when_nav_url_empty() {
        let item = Item {
            url: "https://x.test/p".to_string(),
            canon: "https://x.test/p".to_string(),
            depth: 2,
        };
        let nav = Nav {
            status: 200,
            ..Default::default()
        };
        let rec = record_of(&item, &nav);
        assert_eq!(rec.url, "https://x.test/p");
        assert_eq!(rec.depth, 2);
    }

    struct Flaky {
        hits: std::sync::Mutex<u32>,
        fail_with: u16,
        until: u32,
    }
    #[async_trait]
    impl Navigator for Flaky {
        async fn goto(&self, url: &str) -> Result<Nav, String> {
            let mut h = self.hits.lock().unwrap();
            *h += 1;
            let status = if *h <= self.until {
                self.fail_with
            } else {
                200
            };
            Ok(Nav {
                url: url.to_string(),
                status,
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn retries_retryable_status_then_succeeds() {
        let nav = Arc::new(Flaky {
            hits: std::sync::Mutex::new(0),
            fail_with: 503,
            until: 1, // first attempt 503, retry → 200
        });
        let o = CrawlOptions {
            backoff_ms: 0,
            ..opts(&["https://x.test/"])
        };
        let recs = crawl(o, nav).await;
        assert_eq!(recs[0].status, 200);
    }

    // Navigator that yields at goto so two workers interleave deterministically.
    struct YieldSite;
    #[async_trait]
    impl Navigator for YieldSite {
        async fn goto(&self, url: &str) -> Result<Nav, String> {
            tokio::task::yield_now().await;
            Ok(nav(url, &[]))
        }
    }

    #[tokio::test]
    async fn cap_hit_during_publish_stops_worker() {
        // Two hosts, both claimed (and awaiting goto) before either publishes;
        // with max_pages=1 the second publish sees the cap → returns true →
        // the worker stops (the publish-true / worker-return path).
        let o = CrawlOptions {
            max_pages: 1,
            concurrency: 2,
            start: vec!["https://a.test/".to_string(), "https://b.test/".to_string()],
            ..Default::default()
        };
        let recs = crawl(o, Arc::new(YieldSite)).await;
        assert_eq!(recs.len(), 1);
    }

    struct DenyPrivate;
    #[async_trait]
    impl RobotsGate for DenyPrivate {
        async fn allowed(&self, url: &str, _ua: &str, _now: u64) -> bool {
            !url.contains("/private")
        }
        async fn crawl_delay(&self, _origin: &str, _ua: &str, _now: u64) -> Option<f64> {
            None
        }
    }

    #[tokio::test]
    async fn robots_gate_skips_disallowed() {
        let mut pages = HashMap::new();
        pages.insert(
            "https://x.test/".to_string(),
            nav(
                "https://x.test/",
                &["https://x.test/ok", "https://x.test/private"],
            ),
        );
        pages.insert(
            "https://x.test/ok".to_string(),
            nav("https://x.test/ok", &[]),
        );
        pages.insert(
            "https://x.test/private".to_string(),
            nav("https://x.test/private", &[]),
        );
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            robots: Some(Arc::new(DenyPrivate)),
            ..opts(&["https://x.test/"])
        };
        let mut recs = crawl(o, site).await;
        recs.sort_by(|a, b| a.url.cmp(&b.url));
        let urls: Vec<_> = recs.iter().map(|r| r.url.as_str()).collect();
        // /private fetched-then-gated out; root + /ok remain
        assert_eq!(urls, vec!["https://x.test/", "https://x.test/ok"]);
    }

    struct RobotsStub;
    #[async_trait]
    impl crate::robots::RobotsFetcher for RobotsStub {
        async fn fetch_text(&self, _url: &str) -> Result<(u16, String), ()> {
            Ok((
                200,
                "User-agent: *\nDisallow: /private\nCrawl-delay: 0\n".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn shared_robots_cache_integration() {
        // Drive the real RobotsCache through SharedRobots (the adapter the public
        // API exposes), not just a hand-rolled gate.
        use crate::robots::RobotsCache;
        let gate = SharedRobots::new(RobotsCache::new(RobotsStub));
        let mut pages = HashMap::new();
        pages.insert(
            "https://x.test/".to_string(),
            nav(
                "https://x.test/",
                &["https://x.test/ok", "https://x.test/private/p"],
            ),
        );
        pages.insert(
            "https://x.test/ok".to_string(),
            nav("https://x.test/ok", &[]),
        );
        pages.insert(
            "https://x.test/private/p".to_string(),
            nav("https://x.test/private/p", &[]),
        );
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            robots: Some(Arc::new(gate)),
            ..opts(&["https://x.test/"])
        };
        let mut recs = crawl(o, site).await;
        recs.sort_by(|a, b| a.url.cmp(&b.url));
        let urls: Vec<_> = recs.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x.test/", "https://x.test/ok"]);
    }

    #[tokio::test]
    async fn robots_crawl_delay_folds_into_politeness() {
        // A robots gate advertising a crawl-delay must not break the crawl; the
        // delay is virtualized via the host politeness gate.
        struct Delay;
        #[async_trait]
        impl RobotsGate for Delay {
            async fn allowed(&self, _u: &str, _a: &str, _n: u64) -> bool {
                true
            }
            async fn crawl_delay(&self, _o: &str, _a: &str, _n: u64) -> Option<f64> {
                Some(0.0) // 0s → no real wait, just exercises the fold path
            }
        }
        let mut pages = HashMap::new();
        pages.insert("https://x.test/".to_string(), nav("https://x.test/", &[]));
        let site = Arc::new(Site { pages });
        let o = CrawlOptions {
            robots: Some(Arc::new(Delay)),
            ..opts(&["https://x.test/"])
        };
        assert_eq!(crawl(o, site).await.len(), 1);
    }

    #[tokio::test]
    async fn gives_up_when_retry_budget_zero() {
        let nav = Arc::new(Flaky {
            hits: std::sync::Mutex::new(0),
            fail_with: 503,
            until: 99,
        });
        let o = CrawlOptions {
            backoff_ms: 0,
            retry_budget: 0, // should_retry_status false on the first 503
            ..opts(&["https://x.test/"])
        };
        let recs = crawl(o, nav).await;
        assert_eq!(recs[0].status, 503);
    }

    #[tokio::test]
    async fn shared_robots_gate_delegates_allowed_and_crawl_delay() {
        use crate::robots::RobotsCache;
        let gate: Box<dyn RobotsGate> =
            Box::new(SharedRobots::new(RobotsCache::new(RobotsStub)));
        assert!(gate.allowed("https://x.test/ok", "ua", 0).await);
        assert!(!gate.allowed("https://x.test/private/p", "ua", 0).await);
        let _ = gate.crawl_delay("https://x.test", "ua", 0).await;
    }
}
