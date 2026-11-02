//! turbo-surf#120: DOMMatrix keeps Blink's axis-aligned fast path. Chrome 156
//! (156.0.8078.47, headless, this script on a localhost page) stores a matrix built only
//! from 2D translations and scales (from `new DOMMatrix()`, `"none"` or a
//! translate/scale/zero-turn CSS string) as a float scale plus a float translation,
//! so a NaN stays in its own member, and a non-trailing `undefined` scaleY is NaN.
//! Sequences, dictionaries, `matrix()`, clones, setters, multiply, flips, turns and
//! 3D steps use the full matrix, where NaN spreads through rows and columns.
//! Unit scales, zero translations and zero turns/skews leave the matrix unchanged.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"
const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name+': '+e.message)}};
const L=m=>m.is2D+' '+Array.from(m.toFloat64Array()).join();
const N=m=>L(m.scale(2,undefined,1,5,6));
T(()=>L(new DOMMatrix().scale(2,undefined,1,5,6)));
T(()=>L(new DOMMatrix().scaleSelf(2,undefined,undefined,1)));
T(()=>L(new DOMMatrix().scale(undefined,undefined,1)));
T(()=>L(new DOMMatrix([1,2,3,4,5,6]).scale(2,undefined,1,5,6)));
T(()=>L(new DOMMatrix([2,0,0,3,7,8]).scaleSelf(2,undefined,1,5,6)));
T(()=>L(new DOMMatrix().scale(2,NaN,1)));
T(()=>L(new DOMMatrix().scale(2,undefined,2,1,1,1)));
T(()=>L(new DOMMatrix().scale(NaN)));
T(()=>L(new DOMMatrix().scale(2,undefined,undefined,undefined,undefined,undefined)));
T(()=>L(new DOMMatrix().scale(NaN,1,1,3,4)));
T(()=>L(new DOMMatrix().rotate(30).scale(2,NaN)));
T(()=>L(new DOMMatrix().translate(NaN,1)));
T(()=>L(new DOMMatrix().translate(3,4).scale(1,NaN)));
T(()=>L(new DOMMatrix().scaleNonUniform(2,NaN)));
T(()=>L(new DOMMatrix().scale(2,undefined,1).translate(1,1)));
T(()=>L(new DOMMatrix().scale(2,undefined,1).multiply(new DOMMatrix([1,0,0,1,1,1]))));
T(()=>L(new DOMMatrix().scale(0.1).scale(0.3).translate(0.7,0.9)));
T(()=>L(new DOMMatrix().scale(0.3).translate(0.7).inverse()));
T(()=>L(new DOMMatrix().scaleNonUniform(2,NaN).translate(3,4)));
T(()=>L(new DOMMatrix().translate(NaN,NaN).scale(0)));
T(()=>L(new DOMMatrix().scale(Infinity,0).translate(0,Infinity)));
T(()=>L(new DOMMatrix().translate(1,2).scale(2,undefined,1,5,6).multiply(new DOMMatrix().scale(1,undefined))));
T(()=>L(new DOMMatrix().scale(2,NaN).invertSelf()));
T(()=>L(new DOMMatrix().scale(Infinity).inverse()));
T(()=>L(new DOMMatrix().scale(1e-39).inverse()));
T(()=>L(new DOMMatrix().scale(1e-30,4).translate(3,5).inverse()));
T(()=>L(new DOMMatrix().scale(0.1,undefined)));
T(()=>L(new DOMMatrix().scaleNonUniform(2,undefined)));
T(()=>L(new DOMMatrix().scale3d(2,undefined)));
T(()=>L(new DOMMatrix().translate(1,undefined,2)));
T(()=>L(new DOMMatrix("scale(0.1) scale(0.3) translate(0.7px, 0.9px)")));
T(()=>N(new DOMMatrix("scale(3)")));
T(()=>N(new DOMMatrix("translate(3px,4px)")));
T(()=>N(new DOMMatrix("none")));
T(()=>N(new DOMMatrix([1,0,0,1,0,0])));
T(()=>N(DOMMatrix.fromMatrix({})));
T(()=>N(DOMMatrix.fromMatrix(new DOMMatrix())));
T(()=>N(new DOMMatrix().translate(1,2)));
T(()=>N(new DOMMatrix().translate(1,2,0)));
T(()=>N(new DOMMatrix().scale(3,4,1,0,0,0)));
T(()=>N(new DOMMatrix().scale3d(1)));
T(()=>N(new DOMMatrix().multiply(new DOMMatrix())));
T(()=>N(new DOMMatrix().preMultiplySelf(new DOMMatrix())));
T(()=>N(new DOMMatrix().inverse()));
T(()=>{const m=new DOMMatrix();m.m11=1;return N(m)});
T(()=>{const m=new DOMMatrix();m.a=1;return N(m)});
T(()=>N(new DOMMatrix().flipX()));
T(()=>N(new DOMMatrix().rotate(0)));
T(()=>N(new DOMMatrix().skewX(0)));
T(()=>N(new DOMMatrix().rotateAxisAngle(0,0,1,0)));
T(()=>N(DOMMatrix.fromFloat64Array(new Float64Array([1,0,0,1,0,0]))));
T(()=>N(new DOMMatrixReadOnly()));
T(()=>N(new DOMMatrix().setMatrixValue("scale(2)")));
T(()=>N(structuredClone(new DOMMatrix())));
T(()=>N(new DOMMatrix().rotate(90)));
T(()=>N(new DOMMatrix().rotate(0,0,0)));
T(()=>N(new DOMMatrix().rotateFromVector(1,0)));
T(()=>N(new DOMMatrix().multiply(new DOMMatrix().scale(2))));
T(()=>N(new DOMMatrix().scale(1,1,2)));
T(()=>N(new DOMMatrix().translate(0,0,1)));
T(()=>N(new DOMMatrix().translate(0,0,0)));
T(()=>N(new DOMMatrix("scale(2) translate(1px,2px)")));
T(()=>N(new DOMMatrix("rotate(0deg)")));
T(()=>N(new DOMMatrix("matrix(1,0,0,1,0,0)")));
T(()=>N(new DOMMatrix("scaleX(2) translateY(3px)")));
T(()=>N(new DOMMatrix("translate3d(1px,2px,0)")));
T(()=>N(new DOMMatrix("scale3d(1,2,1)")));
T(()=>N(new DOMMatrix("")));
T(()=>N(new DOMMatrix(undefined)));
T(()=>N(new DOMMatrix().setMatrixValue("matrix(1,0,0,1,0,0)")));
T(()=>N(new DOMMatrix([1,0,0,1,0,0]).setMatrixValue("scale(2)")));
T(()=>N(new DOMMatrix().translate(1,1).scale(0.5).invertSelf()));
T(()=>N(new DOMMatrix("translate(1px) translateZ(1px)")));
T(()=>N(new DOMMatrix("translateZ(0px) rotateZ(0deg) scaleZ(1)")));
T(()=>N(new DOMMatrix("rotateX(0deg)")));
T(()=>N(new DOMMatrix().rotateAxisAngle(1,0,0,0)));
T(()=>N(new DOMMatrix().scale3d(1,1,1,1)));
globalThis.out=out;
"#;

const CHROME_156: &str = r#"["true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-1,NaN,0,1", "true 1,0,0,0,0,NaN,0,0,0,0,1,0,0,0,0,1", "true 2,4,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 4,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,0,0,0,1", "false 2,0,0,0,NaN,NaN,NaN,NaN,0,0,2,0,NaN,NaN,NaN,NaN", "true NaN,0,0,0,0,NaN,0,0,0,0,1,0,0,0,0,1", "true 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1", "true NaN,0,0,0,0,1,0,0,0,0,1,0,NaN,0,0,1", "true 1.7320508075688774,0.9999999999999999,0,0,NaN,NaN,NaN,NaN,0,0,1,0,0,0,0,1", "true 1,0,0,0,0,1,0,0,0,0,1,0,NaN,1,0,1", "true 1,0,0,0,0,NaN,0,0,0,0,1,0,3,4,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,0,0,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,2,NaN,0,1", "true 2,NaN,0,0,0,NaN,0,0,0,NaN,1,0,2,NaN,0,1", "true 0.030000001192092896,0,0,0,0,0.030000001192092896,0,0,0,0,1,0,0.020999999716877937,0.027000000700354576,0,1", "true 3.3333332538604736,0,0,0,0,3.3333332538604736,0,0,0,0,1,0,-0.699999988079071,0,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,6,NaN,0,1", "true 0,0,0,0,0,0,0,0,0,0,1,0,NaN,NaN,0,1", "true Infinity,0,0,0,0,0,0,0,0,0,1,0,NaN,NaN,0,1", "true 2,NaN,0,0,0,NaN,0,0,0,NaN,1,0,-4,NaN,0,1", "false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN", "false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN", "false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN", "true 1.0000000150474662e+30,0,0,0,0,0.25,0,0,0,0,1,0,-3,-5,0,1", "true 0.10000000149011612,0,0,0,0,0.10000000149011612,0,0,0,0,1,0,0,0,0,1", "true 2,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "false 2,0,0,0,0,2,0,0,0,0,2,0,0,0,0,1", "false 1,0,0,0,0,1,0,0,0,0,1,0,1,0,2,1", "true 0.030000001192092896,0,0,0,0,0.030000001192092896,0,0,0,0,1,0,0.020999999716877937,0.027000000700354576,0,1", "true 6,0,0,0,0,NaN,0,0,0,0,1,0,-15,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-2,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-4,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-4,NaN,0,1", "true 6,0,0,0,0,NaN,0,0,0,0,1,0,-15,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true -2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 4,0,0,0,0,NaN,0,0,0,0,1,0,-10,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 0,2,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 4,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "false 2,0,0,0,NaN,NaN,NaN,NaN,0,0,2,0,NaN,NaN,NaN,NaN", "false 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 4,0,0,0,0,NaN,0,0,0,0,1,0,-8,NaN,0,1", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 4,0,0,0,0,NaN,0,0,0,0,1,0,-10,NaN,0,1", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-4,NaN,0,1", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "true 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "true 4,0,0,0,0,NaN,0,0,0,0,1,0,-10,NaN,0,1", "true 4,0,0,0,0,NaN,0,0,0,0,1,0,-12,NaN,0,1", "false 2,0,0,0,NaN,NaN,NaN,NaN,0,0,1,0,NaN,NaN,NaN,NaN", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1", "false 2,0,0,0,0,NaN,0,0,0,0,1,0,-5,NaN,0,1"]"#;

#[tokio::test(flavor = "current_thread")]
async fn axis_aligned_matrices_keep_nan_per_member_like_chrome() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Vec<String> =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let want: Vec<String> = serde_json::from_str(CHROME_156).unwrap();
    let lines: Vec<&str> = SCRIPT
        .lines()
        .filter(|line| line.starts_with("T("))
        .collect();
    let mut wrong = Vec::new();
    for (index, (got, want)) in got.iter().zip(&want).enumerate() {
        if got != want {
            wrong.push(format!("{}\n  got  {got}\n  want {want}", lines[index]));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(got.len(), want.len());
}
