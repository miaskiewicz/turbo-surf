//! `data:` image sources decode natively: PNG and SVG report their intrinsic size, a
//! corrupt or non-image payload fires error. Expected: Chrome 156 (156.0.8078.4,
//! headless) running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(async()=>{const log=[];const load=src=>new Promise(r=>{const i=new Image();i.onload=()=>r(['load',i.naturalWidth,i.naturalHeight,i.complete]);i.onerror=()=>r(['error',i.naturalWidth,i.complete]);i.src=src;});
log.push(await load('data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAEUlEQVR4nGP4z8DwH4YZkDkAm34L9XKwuTwAAAAASUVORK5CYII='));
log.push(await load('data:image/png;base64,AAAA'));
log.push(await load('data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="7" height="5"/>'));
log.push(await load('data:text/plain,hello'));
return log;})()"#;

const CHROME_156: &str =
    r#"[["load",3,2,true],["error",0,true],["load",7,5,true],["error",0,true]]"#;

#[tokio::test(flavor = "current_thread")]
async fn data_url_images_load_or_fail_like_chrome() {
    let mut page = PageSession::open("<body>", "https://images.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(SCRIPT).await.unwrap(), CHROME_156);
}
