//! Producer fixes must survive the full surf bootstrap and its official web interfaces.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn page(html: &str) -> PageSession {
    PageSession::open(html, "http://127.0.0.1/", "", "", 20_000)
        .await
        .unwrap()
}

async fn evaluate(page: &mut PageSession, source: &str) -> Value {
    serde_json::from_str(&page.eval_json_expression(source).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_event_creation_survives_integrated_bootstrap() {
    let mut page = page("<!doctype html><body><input id=field>").await;
    let actual = evaluate(
        &mut page,
        r#"(() => {
      const target=document.getElementById('field'), seen=[];
      target.addEventListener('react-test', event => seen.push([event.type,event.target===target]));
      const states=[];
      for(const alias of ['Event','Events','HTMLEvents']) {
        const event=document.createEvent(alias);
        event.initEvent('react-test',true,true);
        states.push([event.type,event.bubbles,event.cancelable,event instanceof Event]);
        target.dispatchEvent(event);
      }
      target.addEventListener('modern-test',()=>seen.push(['modern-test',true]));
      target.dispatchEvent(new Event('modern-test'));
      return {states,seen};
    })()"#,
    )
    .await;
    assert_eq!(
        actual,
        json!({
            "states": [["react-test",true,true,true],["react-test",true,true,true],["react-test",true,true,true]],
            "seen": [["react-test",true],["react-test",true],["react-test",true],["modern-test",true]]
        })
    );
    page.close();
}

