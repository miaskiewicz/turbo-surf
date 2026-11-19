//! Platform values keep the real frontend's native owner when queued by page code.
use serde_json::json;
use turbo_surf_render::PageSession;

async fn check_value(kind: &str, borrowed: bool) {
    let mut page = PageSession::open(
        "<body><iframe id='child' srcdoc='<body>child</body>'></iframe></body>",
        "https://platform-owner.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let config = json!({"kind":kind,"borrowed":borrowed});
    let body = r#"
      const {kind,borrowed}=config;
      const realm=borrowed?document.querySelector('#child').contentWindow:globalThis;
      const value=kind==='AudioBuffer'
        ?new realm.AudioBuffer({length:8,numberOfChannels:1,sampleRate:48000})
        :new realm[kind](1,2,3,4);
      const stream=new ReadableStream({start(controller){controller.enqueue(value);controller.close();}});
      const read=await stream.getReader().read();
      return {same:read.value===value,
        prototype:Object.getPrototypeOf(value)===realm[kind].prototype,
        functionOwner:value.constructor.constructor('return globalThis')()===realm};
    "#;
    let expression = format!("(async()=>{{const config={config};{body}}})()");
    let actual = page.eval_json_expression(&expression).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        json!({"same":true,"prototype":true,"functionOwner":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dom_rect_keeps_native_guest_owner_and_queue_identity() {
    check_value("DOMRect", false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn borrowed_dom_rect_keeps_native_owner_and_queue_identity() {
    check_value("DOMRect", true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn dom_point_keeps_native_guest_owner_and_queue_identity() {
    check_value("DOMPoint", false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn borrowed_dom_point_keeps_native_owner_and_queue_identity() {
    check_value("DOMPoint", true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn audio_buffer_keeps_native_guest_owner_and_queue_identity() {
    check_value("AudioBuffer", false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn borrowed_audio_buffer_keeps_native_owner_and_queue_identity() {
    check_value("AudioBuffer", true).await;
}
