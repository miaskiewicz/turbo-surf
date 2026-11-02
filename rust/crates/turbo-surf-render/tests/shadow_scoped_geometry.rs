//! Shadow-root stylesheets with `:host`, `::slotted` and descendant / child / next- /
//! later-sibling combinators reach native live layout. Expected box sizes: Chrome 156
//! (156.0.8078.4, headless) running the same page on localhost.
use turbo_surf_render::PageSession;

const PAGE: &str = r#"<!doctype html><body><div id=host class=h><em slot=s>e</em></div><script>
try{
const host=document.getElementById('host');const root=host.attachShadow({mode:'open'});
root.innerHTML=`<style>
span,b,i,u,q{display:block;width:1px;height:1px}
:host div span{width:11px}
:host p + b{width:12px}
:host p ~ i{width:13px}
:host(.h) section > u{width:14px}
:host(:not(.zz)) q{width:15px}
::slotted(em){display:block;width:16px;height:1px}
:host p + i{height:7px}
</style><div><article><span id=a></span></article></div><p></p><b id=b></b><s></s><i id=c></i><section><u id=d></u></section><q id=e></q><slot name=s></slot><b id=f></b>`;
const w=el=>[el.getBoundingClientRect().width,el.getBoundingClientRect().height];
globalThis.result=JSON.stringify(['a','b','c','d','e','f'].map(id=>w(root.getElementById(id))).concat([w(host.querySelector('em'))]));
}catch(e){globalThis.result=String(e&&e.stack||e)}
</script></body>"#;

#[tokio::test(flavor = "current_thread")]
async fn shadow_combinators_size_boxes_like_chrome() {
    let mut page = PageSession::open(PAGE, "https://scoped.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression("JSON.parse(result)")
            .await
            .unwrap(),
        "[[11,1],[12,1],[13,1],[14,1],[15,1],[1,1],[16,1]]"
    );
}
