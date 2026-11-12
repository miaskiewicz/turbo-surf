#[tokio::test(flavor = "current_thread")]
async fn initial_frame_configuration_does_not_read_parent_geometry() {
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><head></head><body></body>",
        "https://startup.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(async () => {
                  const foreign = document.createElementNS('urn:fixture', 'meta');
                  document.head.append(foreign);
                  const frame = document.createElement('iframe');
                  frame.style = 'width:200px;height:100px;border:0';
                  frame.srcdoc = '<script>globalThis.started=true</script>';
                  const loaded = new Promise(resolve => frame.addEventListener('load', resolve, {once:true}));
                  document.body.append(frame);
                  const initialDocument = frame.contentWindow.document;
                  const initialRealmReady = initialDocument.defaultView === frame.contentWindow && typeof frame.contentWindow.Array === "function";
                  await loaded;
                  const started = frame.contentWindow.started;
                  let geometryError = '';
                  try { void frame.contentWindow.innerWidth; } catch (error) { geometryError = error.message; }
                  const namespace = foreign.namespaceURI;
                  foreign.remove();
                  const width = frame.contentWindow.innerWidth;
                  frame.remove();
                  return {started, initialRealmReady, geometryError, namespace, width};
                })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual["started"], true);
    assert_eq!(actual["initialRealmReady"], true);
    assert_eq!(actual["namespace"], "urn:fixture");
    assert_eq!(
        actual["geometryError"],
        "native live layout does not yet support this element namespace"
    );
    assert_eq!(actual["width"], 200);
    page.close();
}
