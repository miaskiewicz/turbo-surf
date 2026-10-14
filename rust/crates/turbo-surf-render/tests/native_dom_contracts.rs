//! Programmatic activation and private native Range contracts.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;
async fn evaluate(source: &str) -> Value {
    let mut page = PageSession::open(
        "<!doctype html><body><p>abcdef</p></body>",
        "https://dom.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_expression(source).await.unwrap();
    page.close();
    serde_json::from_str(&result).unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn native_anchor_activation_honors_cancellation_detachment_and_repeated_downloads() {
    let value=evaluate(r#"(()=>{
      const anchor=document.createElement('a'),url=URL.createObjectURL(new Blob(['actual bytes']));anchor.href=url;anchor.download='fixture.txt';document.body.appendChild(anchor);
      const records=globalThis[Symbol.for('ts')].downloads ||= [];const before=records.length;
      let events=[],recursive=false;const cancel=event=>event.preventDefault();anchor.addEventListener('click',cancel);anchor.click();anchor.removeEventListener('click',cancel);
      anchor.addEventListener('click',event=>{events.push({brand:Object.prototype.toString.call(event),trusted:event.isTrusted,bubbles:event.bubbles,cancelable:event.cancelable,composed:event.composed,pointerId:event.pointerId});if(!recursive){recursive=true;anchor.click();recursive=false;}});
      const result=anchor.click();anchor.click();anchor.remove();anchor.click();
      const downloaded=records.slice(before);
      return JSON.stringify({cancelled:downloaded.length===3,count:events.length,events,downloads:downloaded.map(record=>({filename:record.filename,content:record.content})),returnsUndefined:result===undefined,native:Function.prototype.toString.call(anchor.click).includes('[native code]'),inherits:anchor.click===HTMLElement.prototype.click&&!Object.hasOwn(HTMLAnchorElement.prototype,'click')});
    })()"#).await;
    for name in ["cancelled", "returnsUndefined", "native", "inherits"] {
        assert_eq!(value[name], true, "{value}");
    }
    assert_eq!(value["count"], 3);
    assert_eq!(
        value["downloads"],
        json!([{"filename":"fixture.txt","content":"actual bytes"},{"filename":"fixture.txt","content":"actual bytes"},{"filename":"fixture.txt","content":"actual bytes"}])
    );
    for event in value["events"].as_array().unwrap() {
        assert_eq!(
            event,
            &json!({"brand":"[object PointerEvent]","trusted":false,"bubbles":true,"cancelable":true,"composed":true,"pointerId":-1})
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn native_range_clones_hide_state_and_preserve_independent_live_node_references() {
    let value=evaluate(r#"(()=>{
      const range=document.createRange(),text=document.querySelector('p').firstChild;range.setStart(text,1);range.setEnd(text,4);const copy=range.cloneRange();copy.collapse(true);
      let cloneError,forged;try{structuredClone(range);}catch(e){cloneError=e.name;}try{Range.prototype.cloneRange.call(Object.create(Range.prototype));}catch(e){forged=e.name;}
      return JSON.stringify({own:Object.keys(range),brand:Object.prototype.toString.call(copy),bounds:[range.startOffset,range.endOffset,copy.startOffset,copy.endOffset],sameNodes:range.startContainer===copy.startContainer,constructor:copy.constructor===Range,native:Function.prototype.toString.call(copy.cloneRange).includes('[native code]'),cloneError,forged,private:typeof __nativeRangeRecord==='undefined'});
    })()"#).await;
    assert_eq!(value["own"], json!([]));
    assert_eq!(value["brand"], "[object Range]");
    assert_eq!(value["bounds"], json!([1, 4, 1, 1]));
    assert_eq!(value["cloneError"], "DataCloneError");
    assert_eq!(value["forged"], "TypeError");
    for name in ["sameNodes", "constructor", "native", "private"] {
        assert_eq!(value[name], true, "{value}");
    }
}
