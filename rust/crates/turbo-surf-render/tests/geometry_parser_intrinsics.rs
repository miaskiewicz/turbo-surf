//! The CSS transform parser behind `new DOMMatrix(string)` / `setMatrixValue()`
//! keeps working after the page replaces the String/RegExp/Array methods it
//! would otherwise call at use time (turbo-surf#82): the same inputs give the same
//! results while they are patched. Expected values: Chrome 156 (156.0.8078.47,
//! headless) running the script on a localhost page.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const out={};
const inputs=['translate(3px, 4px) rotate(30deg)','  matrix(1,2,3,4,5,6)  ','none','',' scale(2) translateX(1PX) skewY(10deg)','rotate3d(1,1,0,0.5turn) perspective(100px)','matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,7,8,9,1)','TRANSLATEX(2px)','rotate(1.5E1DEG) skewx(45DEG)','translateY(-.5px) scale(+2,1e1)'];
const invalid=['bogus(1)','rotate(1)','scale(1, 2, 3)','translate(1em)','rotate(1deg),scale(2)','rotate (1deg)','rotate(a(b))','x-rotate(1deg)','rotate(1e)','rotate(5.deg) junk'];
const run=()=>inputs.map(function(t){try{return new DOMMatrix(t).toString()}catch(e){return e.name+':'+e.message}}).concat(invalid.map(function(t){try{new DOMMatrix(t);return 'ok'}catch(e){return e.name}}),[(()=>{const m=new DOMMatrix();m.setMatrixValue('translate(5px,6px)');return m.e})()]);
const want=JSON.stringify(run());
const names=[[String.prototype,'trim'],[String.prototype,'slice'],[String.prototype,'split'],[String.prototype,'matchAll'],[String.prototype,'toLowerCase'],[String.prototype,'replace'],[String.prototype,'indexOf'],[String.prototype,'charCodeAt'],[String.prototype,'substring'],[RegExp.prototype,'exec'],[RegExp.prototype,'test'],[RegExp.prototype,Symbol.matchAll],[RegExp.prototype,Symbol.split],[RegExp.prototype,Symbol.replace],[RegExp.prototype,Symbol.match],[window,'String'],[window,'RegExp'],[Array.prototype,'push'],[Array.prototype,Symbol.iterator]];
const saved=[];for(let i=0;i<names.length;i++)saved[i]=names[i][0][names[i][1]];
let got;
for(let i=0;i<names.length;i++){const k=names[i][1];names[i][0][k]=function(){throw new Error('patched '+(typeof k==='symbol'?k.description:k))}}
try{got=JSON.stringify(run())}catch(x){got='threw '+String(x&&x.message)}
finally{for(let i=0;i<names.length;i++)names[i][0][names[i][1]]=saved[i]}
out.values=JSON.parse(want);out.hostile=got===want?'same':got;
globalThis.out=out;"#;

const CHROME_156: &str = r#"{"values":["matrix(0.8660254037844387, 0.49999999999999994, -0.49999999999999994, 0.8660254037844387, 3, 4)","matrix(1, 2, 3, 4, 5, 6)","matrix(1, 0, 0, 1, 0, 0)","matrix(1, 0, 0, 1, 0, 0)","matrix(2, 0.35265396141692995, 0, 2, 2, 0)","matrix3d(-2.220446049250313e-16, 0.9999999999999998, 0, 0, 0.9999999999999998, -2.220446049250313e-16, 0, 0, 0, 0, -1, -0.01, 0, 0, 0, 1)","matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 7, 8, 9, 1)","matrix(1, 0, 0, 1, 2, 0)","matrix(0.9659258262890683, 0.25881904510252074, 0.7071067811865475, 1.224744871391589, 0, 0)","matrix(2, 0, 0, 10, 0, -0.5)","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError","SyntaxError",5],"hostile":"same"}"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#82: fails until the transform parser captures its intrinsics"]
async fn transform_parser_ignores_page_patched_intrinsics_like_chrome156() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    assert_eq!(got["hostile"], chrome["hostile"]);
    assert_eq!(got, chrome);
}
