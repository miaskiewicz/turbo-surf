//! turbo-surf#117: one page rule with a pseudo-class the native matcher does not
//! support must not abort layout or geometry. Chrome drops (or never matches) such
//! a rule and lays out the rest of the page, so `#a` measures 7px and a click on
//! `#b` reaches its listener.
use turbo_surf_render::PageSession;

const SELECTORS: &[&str] = &[
    "div:bogus-pseudo",
    "div:-webkit-autofill",
    "input:-webkit-autofill",
    "div:-moz-focusring",
    "div:-webkit-any-link",
    "a:-webkit-any-link",
    "div:-internal-list-box",
    "div:popover-open",
    "div:state(open)",
    "div:xr-overlay",
    "div:picture-in-picture",
    "div:fullscreen",
    "div:modal",
    "div:open",
    "div:user-invalid",
    "div:autofill",
    "div:has-slotted",
    "div:target-current",
    "div:playing",
    "div:muted",
    "div:-webkit-full-screen",
    "div:window-inactive",
    "div:horizontal",
    "div:dir(rtl)",
    ":host-context(.x) div",
    "div:active-view-transition",
    "div:heading",
    "div:current",
    "div:past",
    "div:local-link",
    ":is(div:bogus-pseudo, span)",
    ":where(:bogus-pseudo) div",
    "div:not(:bogus-pseudo)",
    "div:has(:bogus-pseudo)",
    "div:nth-child(2 of :bogus-pseudo)",
];

async fn width_with(selector: &str) -> Result<String, String> {
    let html = format!(
        "<style>.t{{height:1px;width:5px}} {selector}{{color:red}} #a{{width:7px}}</style>\
         <body><div id=a class=t></div><button id=b>b</button>"
    );
    let mut page = PageSession::open(&html, "http://localhost/", "", "", 10000)
        .await
        .map_err(|e| format!("{e:?}"))?;
    page.eval_json_expression("document.getElementById('a').getBoundingClientRect().width")
        .await
        .map_err(|e| format!("{e:?}"))
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_pseudo_class_rules_leave_layout_running() {
    let mut failures = Vec::new();
    for selector in SELECTORS {
        match width_with(selector).await {
            Ok(width) if width == "7" => {}
            other => failures.push(format!("{selector}: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

const SHADOW_SELECTORS: &[&str] = &[
    "div:bogus-pseudo",
    ":host(:bogus-pseudo)",
    ":host:bogus-pseudo",
    ":host(.x) div:-webkit-autofill",
    ":is(:host) div:bogus-pseudo",
    ":host > div:popover-open",
    "::slotted(div:bogus-pseudo)",
    ":host-context(.x)",
    ":is(div:bogus-pseudo, span)",
    ":where(:bogus-pseudo) div",
    ":not(:bogus-pseudo)",
    ":has(:bogus-pseudo)",
    "div:-webkit-any-link",
    "div:target-current",
    ":host > div:target-current",
];

async fn shadow_width_with(selector: &str) -> Result<String, String> {
    let html = "<body><div id=h><div id=a class=t></div></div>";
    let mut page = PageSession::open(html, "http://localhost/", "", "", 10000)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let script = format!(
        "(() => {{ const root = document.getElementById('h').attachShadow({{mode:'open'}}); \
         const s = document.createElement('style'); \
         s.textContent = {css:?}; root.append(s, document.createElement('slot')); \
         const inner = document.createElement('div'); inner.id = 'i'; root.append(inner); \
         return [document.getElementById('a').getBoundingClientRect().width, \
                 inner.getBoundingClientRect().width].join(); }})()",
        css = format!("{selector}{{color:red}} #i{{width:9px;height:1px}}")
    );
    page.eval_json_expression(&script)
        .await
        .map_err(|e| format!("{e:?}"))
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_pseudo_class_shadow_rules_leave_layout_running() {
    let mut failures = Vec::new();
    for selector in SHADOW_SELECTORS {
        match shadow_width_with(selector).await {
            Ok(width) if width.contains(",9") => {}
            other => failures.push(format!("{selector}: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
