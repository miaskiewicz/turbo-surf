//! Page script cannot reach the engine namespace by any reflective route
//! (miaskiewicz/turbo-dom#222), and `window` has Chrome 156's Window interface chain with
//! SharedArrayBuffer hidden outside cross-origin isolation (miaskiewicz/turbo-dom#223).
//! Expected values: Chrome 156.0.8078.47 headless on http://localhost.
use turbo_surf_render::PageSession;

const UNREACHABLE: &str = r#"(() => {
  const seen = new Set(), found = [];
  const visit = (o, path) => {
    if (o === null || (typeof o !== 'object' && typeof o !== 'function') || seen.has(o)) return;
    seen.add(o);
    for (const k of Reflect.ownKeys(o)) if (typeof k === 'symbol' && (Symbol.keyFor(k) !== undefined || k.description === 'ts')) found.push(path + '[' + String(k) + ']');
  };
  for (let o = globalThis, i = 0; o; o = Object.getPrototypeOf(o), i++) visit(o, 'chain' + i);
  for (const k of Reflect.ownKeys(globalThis)) {
    let v; try { v = globalThis[k]; } catch (e) { continue; }
    visit(v, String(k));
    if (typeof v === 'function') { try { visit(v.prototype, String(k) + '.prototype'); } catch (e) {} }
  }
  for (let o = document; o; o = Object.getPrototypeOf(o)) visit(o, 'document');
  return {
    symbolFor: [globalThis[Symbol.for('ts')], window[Symbol.for('ts')], self[Symbol.for('ts')]].every(v => v === undefined),
    inOperator: Symbol.for('ts') in globalThis,
    ownSymbols: Object.getOwnPropertySymbols(globalThis).map(String),
    found,
  };
})()"#;

const WINDOW_CHAIN: &str = r#"(() => {
  const tag = o => Object.prototype.hasOwnProperty.call(o, Symbol.toStringTag) ? o[Symbol.toStringTag] : null;
  const chain = [];
  for (let o = Object.getPrototypeOf(window); o; o = Object.getPrototypeOf(o)) chain.push([tag(o), Object.prototype.hasOwnProperty.call(o, 'constructor') ? o.constructor.name : null]);
  let setProto; try { Object.setPrototypeOf(window, {}); setProto = 'ok'; } catch (e) { setProto = e.constructor.name; }
  let call; try { Window(); } catch (e) { call = e.message; }
  let construct; try { new Window(); } catch (e) { construct = e.message; }
  return {
    chain, setProto, call, construct,
    instanceWindow: window instanceof Window, instanceEventTarget: window instanceof EventTarget,
    own: ['addEventListener', 'removeEventListener', 'dispatchEvent'].map(k => Object.hasOwn(window, k)),
    constants: [Window.TEMPORARY, Window.PERSISTENT, Window.prototype.TEMPORARY, Window.prototype.PERSISTENT],
    windowProto: Reflect.ownKeys(Window.prototype).map(String),
    sharedArrayBuffer: typeof SharedArrayBuffer, sabOwn: Object.hasOwn(window, 'SharedArrayBuffer'),
    crossOriginIsolated: self.crossOriginIsolated, tag: Object.prototype.toString.call(window),
  };
})()"#;

async fn page() -> PageSession {
    PageSession::open("<!doctype html><body>", "http://localhost/", "", "", 20000)
        .await
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn page_script_cannot_reach_the_engine_namespace() {
    let mut page = page().await;
    let out = page.eval_json_expression(UNREACHABLE).await.unwrap();
    assert_eq!(
        out,
        r#"{"symbolFor":true,"inOperator":false,"ownSymbols":[],"found":[]}"#
    );
    // Driver scripts still reach it through the engine path.
    assert_eq!(
        page.eval_engine_json_expression("typeof __tsNs.runTimers")
            .await
            .unwrap(),
        r#""function""#
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn window_has_chromes_window_interface_chain() {
    let mut page = page().await;
    let out: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(WINDOW_CHAIN).await.unwrap()).unwrap();
    assert_eq!(
        out,
        serde_json::json!({
            "chain": [["Window", "Window"], ["WindowProperties", null], ["EventTarget", "EventTarget"], [null, "Object"]],
            "setProto": "TypeError",
            "call": "Illegal constructor",
            "construct": "Failed to construct 'Window': Illegal constructor",
            "instanceWindow": true, "instanceEventTarget": true,
            "own": [false, false, false],
            "constants": [0, 1, 0, 1],
            "windowProto": ["TEMPORARY", "PERSISTENT", "constructor", "Symbol(Symbol.toStringTag)"],
            "sharedArrayBuffer": "undefined", "sabOwn": false,
            "crossOriginIsolated": false, "tag": "[object Window]"
        })
    );
    page.close();
}
