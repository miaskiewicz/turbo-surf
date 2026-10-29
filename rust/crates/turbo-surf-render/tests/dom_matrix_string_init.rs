//! The DOMMatrix/DOMMatrixReadOnly constructor takes `(DOMString or
//! sequence<unrestricted double>)`: an object without `@@iterator` (another DOMMatrix,
//! an object with toString) converts to a string and parses as a CSS transform.
//! Chrome 156 (156.0.8078.47, headless, localhost):
//! `new DOMMatrix(new DOMMatrix([1,2,3,4,5,6]))` copies the matrix through its
//! `matrix(...)` text, and `new DOMMatrix(new DOMPoint())` is a SyntaxError.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"
const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name)}};
const L=m=>m.is2D+' '+Array.from(m.toFloat64Array()).join();
T(()=>L(new DOMMatrix(new DOMMatrix([1,2,3,4,5,6]))));
T(()=>L(new DOMMatrixReadOnly(new DOMMatrix().translate(1,2,3))));
T(()=>L(new DOMMatrix({toString(){return 'scale(2)'}})));
T(()=>L(new DOMMatrix(new DOMPoint())));
T(()=>L(new DOMMatrix([1,0,0,1,7,8])));
T(()=>L(new DOMMatrix({[Symbol.iterator]:function*(){yield* [1,0,0,1,3,4]}})));
"#;

const CHROME_156: [&str; 6] = [
    "true 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1",
    "false 1,0,0,0,0,1,0,0,0,0,1,0,1,2,3,1",
    "true 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1",
    "SyntaxError",
    "true 1,0,0,0,0,1,0,0,0,0,1,0,7,8,0,1",
    "true 1,0,0,0,0,1,0,0,0,0,1,0,3,4,0,1",
];

#[tokio::test(flavor = "current_thread")]
async fn non_iterable_objects_initialize_through_their_string() {
    let html = format!("<!doctype html><body><script>{SCRIPT}globalThis.out=out;</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Vec<String> =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    assert_eq!(got, CHROME_156);
}
