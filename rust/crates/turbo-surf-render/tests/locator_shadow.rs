//! Native locator indices observe real tree mutations without changing DOM queries.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn locator_shadow_index_tracks_mutations_and_ignores_closed_roots() {
    let mut page = PageSession::open(
        "<!doctype html><div id=open></div><div id=closed></div>",
        "https://locator.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    assert_eq!(
        page.read_engine("return __tsNs.tcLocatorHasShadow()")
            .unwrap(),
        "false"
    );
    page.eval_expression(
        r#"(() => {
          globalThis.root = document.getElementById('open').attachShadow({mode:'open'});
          root.innerHTML = '<button data-key=first>First</button>';
          document.getElementById('closed').attachShadow({mode:'closed'}).innerHTML = '<button data-key=secret>Secret</button>';
        })()"#,
    )
    .await
    .unwrap();
    let selected = "return __tsNs.tcLocatorQuery(document, '[data-key]').map(element => element.getAttribute('data-key'))";
    assert_eq!(page.read_engine(selected).unwrap(), r#"["first"]"#);
    page.eval_expression("root.innerHTML = '<button data-key=second>Second</button>'")
        .await
        .unwrap();
    assert_eq!(page.read_engine(selected).unwrap(), r#"["second"]"#);
    assert_eq!(
        page.eval_expression("document.querySelectorAll('[data-key]').length")
            .await
            .unwrap(),
        "0"
    );
    page.eval_expression(
        "document.querySelectorAll=()=>[];Element.prototype.querySelectorAll=()=>[];Object.defineProperty(Element.prototype,'shadowRoot',{configurable:true,get(){throw Error('guest getter')}})",
    )
    .await
    .unwrap();
    assert_eq!(page.read_engine(selected).unwrap(), r#"["second"]"#);
    page.eval_expression("document.getElementById('open').remove()")
        .await
        .unwrap();
    assert_eq!(page.read_engine(selected).unwrap(), "[]");
    assert_eq!(
        page.read_engine("return __tsNs.tcLocatorHasShadow()")
            .unwrap(),
        "false"
    );
    page.close();
}
