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

#[tokio::test(flavor = "current_thread")]
async fn title_reads_and_writes_preserve_document_order_with_large_unrelated_body() {
    let html = format!(
        "<!doctype html><head><title> First\n title </title><title>Second</title></head><body>{}",
        "<div><span>Payroll row</span><b>123.45</b></div>".repeat(5_000)
    );
    let mut page = page(&html).await;
    let actual = evaluate(
        &mut page,
        r#"(() => {
      const values=[];
      for(let index=0;index<80;index++) {
        if(document.title!=='First title')throw Error('initial title');
      }
      const first=document.querySelector('title');
      first.remove();values.push(document.title);
      document.body.prepend(first);values.push(document.title);
      document.head.querySelector('title').remove();values.push(document.title);
      document.title=' Updated\t title ';values.push(document.title,first.textContent);
      return values;
    })()"#,
    )
    .await;
    assert_eq!(
        actual,
        json!([
            "Second",
            "Second",
            "First title",
            "Updated title",
            " Updated\t title "
        ])
    );
    page.close();
}
