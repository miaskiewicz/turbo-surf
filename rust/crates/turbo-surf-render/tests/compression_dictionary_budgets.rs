//! Compression-dictionary link discovery runs over every document before its
//! resources load, so its native bounds must never fail the document itself
//! (turbo-surf#75). Chrome 156 opens either page below normally.
use turbo_surf_render::PageSession;

/// More than 100,000 DOM nodes (elements + text) is an ordinary large page: it opens
/// and runs its scripts. Discovery walks it linearly with no node budget.
#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#75: fails until the fix"]
async fn documents_over_a_hundred_thousand_nodes_open() {
    let html = format!(
        "<!doctype html><body>{}<script>document.title='ran'</script>",
        "<i>x</i>".repeat(50_100)
    );
    let mut page = PageSession::open(&html, "https://dictionary.test/", "", "", 60_000)
        .await
        .unwrap();
    let result = page
        .eval_expression("document.title+':'+document.getElementsByTagName('*').length")
        .await
        .unwrap();
    page.close();
    assert_eq!(result, "ran:50105");
}

/// Every compression-dictionary link settles, however many there are: 300 links to
/// one dictionary that the page's CSP blocks each fire `error` (frozen Chrome 156 on
/// https://localhost: [300, 300, true]).
#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#75: fails until the fix"]
async fn every_dictionary_link_settles_past_256_links() {
    let links: String = (0..300)
        .map(|index| format!("<link rel=compression-dictionary href=/dict id=d{index}>"))
        .collect();
    let html = format!(
        "<!doctype html><head><meta http-equiv=Content-Security-Policy content=\"connect-src 'none'\">\
         <script>var ev=[];document.addEventListener('error',e=>ev.push('e'+e.target.id),true);\
         document.addEventListener('load',e=>ev.push('l'+e.target.id),true)</script>{links}</head><body>"
    );
    let mut page = PageSession::open(&html, "https://dictionary.test/", "", "", 20_000)
        .await
        .unwrap();
    let result = page
        .eval_json_expression(
            "new Promise(r=>setTimeout(()=>r([ev.length,ev.filter(x=>x[0]=='e').length,ev.includes('ed299')]),200))",
        )
        .await
        .unwrap();
    page.close();
    assert_eq!(result, "[300,300,true]");
}
