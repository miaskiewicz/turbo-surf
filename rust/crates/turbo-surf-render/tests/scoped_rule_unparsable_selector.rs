//! A page rule whose selector the CSSOM keeps but the native scoped-rule parser
//! (Lightning CSS) rejects, such as `::selection:window-inactive`, must not abort the
//! live layout. Chrome 156 (156.0.8078.47, headless, localhost) keeps the rule (it
//! only styles a selection) and lays the page out: `#a` measures 7px.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn a_rule_the_native_parser_rejects_leaves_layout_running() {
    let html = "<style>.t{height:1px;width:5px} div::selection:window-inactive{color:red} #a{width:7px}</style>\
                <body><div id=a class=t></div>";
    let mut page = PageSession::open(html, "http://localhost/", "", "", 10000)
        .await
        .unwrap();
    let value = page
        .eval_json_expression("document.getElementById('a').getBoundingClientRect().width")
        .await
        .unwrap();
    assert_eq!(value, "7");
}
