use serde_json::{json, Value};
use turbo_surf_render::{run_with_dom, PageSession};

const HTML: &str = include_str!("fixtures/auth_menu_animations.html");
fn setup() -> &'static str {
    HTML.split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0
}

#[test]
fn auth_box_and_leaf_extent_reads_ignore_only_unrelated_paint_effects() {
    let script = format!("{};setAuthPhase('none');const baseline=readAuthGeometry();const phases={{}};for(const phase of ['typing','message']){{setAuthPhase(phase);phases[phase]=readAuthGeometry();}}JSON.stringify({{baseline,phases}})", setup());
    let report: Value = serde_json::from_str(&run_with_dom(HTML, &script).unwrap()).unwrap();
    for phase in ["typing", "message"] {
        for key in [
            "iconRect",
            "triggerRect",
            "rippleRect",
            "rippleClientWidth",
            "rippleClientHeight",
            "textareaUsedWidth",
            "textareaClientWidth",
            "textareaOffsetHeight",
            "shadowScrollHeight",
        ] {
            assert_eq!(
                report["phases"][phase][key], report["baseline"][key],
                "{phase}/{key}: {report}"
            );
        }
        assert!(report["phases"][phase]["regionScrollHeight"]["error"]
            .as_str()
            .unwrap()
            .contains("animation resource"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn auth_hover_and_ripple_use_private_independent_box_centers() {
    let html = format!("{}</body></html>", HTML.split_once("<script>").unwrap().0);
    let mut page = PageSession::open(&html, "https://auth-layout.test/", "", "", 60_000)
        .await
        .unwrap();
    page.eval(setup()).await.unwrap();
    page.eval("document.getElementById('trigger').getBoundingClientRect=()=>{throw Error('guest rectangle override')}").await.unwrap();
    for phase in ["typing", "message"] {
        page.eval(&format!("setAuthPhase('{phase}');authEvents=[];"))
            .await
            .unwrap();
        page.input(&json!([{"move":{"selector":"#trigger"}}]))
            .await
            .unwrap();
        page.input(&json!([{"click":"#logout"}])).await.unwrap();
        let reads: Value =
            serde_json::from_str(&page.eval_json_expression("rippleReads").await.unwrap()).unwrap();
        for key in ["rect", "width", "height"] {
            assert!(reads[key]["value"].is_number(), "{phase}: {reads}");
        }
        assert_eq!(
            page.eval_json_expression("authEvents.some(row=>row[0]==='click'&&row[1]===true)")
                .await
                .unwrap(),
            "true"
        );
        page.eval("document.querySelectorAll('.ripple').forEach(node=>node.remove())")
            .await
            .unwrap();
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn auth_resize_observer_keeps_measuring_textarea_with_background_paint() {
    let html = format!("{}</body></html>", HTML.split_once("<script>").unwrap().0);
    let mut page = PageSession::open(&html, "https://auth-layout.test/", "", "", 60_000)
        .await
        .unwrap();
    page.eval(setup()).await.unwrap();
    let result:Value=serde_json::from_str(&page.eval_json_expression("new Promise(resolve=>{const timer=setTimeout(()=>resolve({missing:true}),2000);const observer=new ResizeObserver(entries=>{clearTimeout(timer);observer.disconnect();resolve({entries:entries.length,width:entries[0].contentRect.width,scroll:recordRead(()=>document.getElementById('shadow').scrollHeight)})});observer.observe(document.getElementById('textarea'))})").await.unwrap()).unwrap();
    assert!(result["entries"].as_u64().unwrap_or(0) > 0, "{result}");
    assert!(result["width"].as_f64().unwrap() > 0.0, "{result}");
    assert!(result["scroll"]["value"].is_number(), "{result}");
    page.close();
}

#[test]
fn local_box_cache_never_admits_extent_hit_test_gutter_or_reparented_effects() {
    let script=format!("{};setAuthPhase('typing');const read=fn=>{{try{{return {{value:fn()}}}}catch(error){{return {{error:error.message}}}}}};const region=document.getElementById('region');const rows=[read(()=>region.getBoundingClientRect().width),read(()=>region.scrollHeight),read(()=>document.elementFromPoint(1,1))];const leaf=document.createElement('div');leaf.style.cssText='overflow:auto;width:100px;height:30px';document.body.append(leaf);rows.push(read(()=>leaf.scrollHeight));const typing=document.getElementById('typing');leaf.append(typing);rows.push(read(()=>leaf.getBoundingClientRect().width),read(()=>leaf.scrollHeight));region.append(typing);rows.push(read(()=>leaf.scrollHeight));document.getElementById('paper').style.setProperty('scrollbar-gutter','stable');rows.push(read(()=>document.getElementById('icon').getBoundingClientRect().width));JSON.stringify(rows)",setup());
    let rows: Value = serde_json::from_str(&run_with_dom(HTML, &script).unwrap()).unwrap();
    for (i, expected) in [(0, 400), (3, 30), (4, 100), (6, 30)] {
        assert_eq!(rows[i]["value"], expected, "{rows}");
    }
    for i in [1, 2, 5, 7] {
        assert!(rows[i]["error"].is_string(), "{rows}");
    }
}

#[test]
fn quirks_body_viewport_extent_remains_strict_after_its_local_box_is_cached() {
    let html="<style>@keyframes move{to{transform:translateY(100px)}}head{display:block;height:100px;animation:move 1s infinite}body{width:80px;height:60px}</style><div>body</div>";
    let actual=run_with_dom(html,"(()=>{const body=document.body;const width=body.getBoundingClientRect().width;try{return JSON.stringify([width,body.scrollHeight])}catch(e){return JSON.stringify([width,e.message])}})()").unwrap();
    let rows: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(rows[0], 80);
    assert!(rows[1].as_str().unwrap().contains("animation resource"));
}
