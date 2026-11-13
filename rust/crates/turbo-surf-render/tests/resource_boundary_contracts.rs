use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn open(html: &str) -> PageSession {
    PageSession::open(html, "https://geometry.test/", "", "", 60_000)
        .await
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn reversed_container_ranges_keep_inclusive_and_exclusive_boundaries() {
    let mut page = open(
        "<!doctype html><style>
        #container{container-type:size;width:300px;height:200px}
        :where(#container)>div{width:10px;height:10px}
        @container (400px > width > 200px){#inside{width:70px}}
        @container (300px >= width >= 300px){#equal{width:70px}}
        @container (300px > width > 200px){#excluded{width:70px}}
        @container (aspect-ratio:1.5){#fraction{width:70px}}
        @container (aspect-ratio:2){#other{width:70px}}
        </style><div id=container><div id=inside></div><div id=equal></div>
        <div id=excluded></div><div id=fraction></div><div id=other></div></div>",
    )
    .await;
    let actual = page
        .eval_json_expression(
            "['inside','equal','excluded','fraction','other'].map(id=>document.getElementById(id).getBoundingClientRect().width)",
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!([70, 70, 10, 70, 10])
    );
    assert!(!page.hydration_diagnostics().budget_exceeded);
}

#[tokio::test(flavor = "current_thread")]
async fn cyclic_paint_variables_do_not_hide_a_later_geometry_dependency() {
    let mut page = open(
        "<!doctype html><style>
        @property --paint{syntax:'<color>';inherits:true;initial-value:red}
        #target{--first:var(--paint,var(--second));--second:var(--first);
          color:var(--second);width:40px;height:20px}
        </style><div id=target></div>",
    )
    .await;
    let before = page
        .eval_json_expression("(()=>{const r=document.getElementById('target').getBoundingClientRect();return [r.width,r.height]})()")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&before).unwrap(),
        json!([40, 20])
    );
    let rejection = page
        .eval_expression("(()=>{const node=document.getElementById('target');node.style.height='var(--second)';try{node.getBoundingClientRect();return 'admitted'}catch(error){return error.message}})()")
        .await
        .unwrap();
    assert!(
        rejection.contains("geometry dependency") && rejection.contains("height"),
        "{rejection}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn quoted_animation_shorthand_keeps_the_geometry_guard() {
    for animation in [
        "animation-name:'wide card';animation-duration:100s",
        "animation:100s linear 'wide card'",
    ] {
        let mut page = open(&format!(
            "<!doctype html><style>@keyframes 'wide card'{{to{{width:100px}}}}
            #target{{width:20px;height:10px;{animation}}}</style><div id=target></div>"
        ))
        .await;
        let rejection = page
            .eval_expression("(()=>{try{document.getElementById('target').getBoundingClientRect();return 'admitted'}catch(error){return error.message}})()")
            .await
            .unwrap();
        assert!(
            rejection.contains("animation resource") && rejection.contains("wide card"),
            "{animation}: {rejection}"
        );
    }
}
