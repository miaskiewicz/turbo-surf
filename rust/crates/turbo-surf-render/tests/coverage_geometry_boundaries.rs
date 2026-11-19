//! Public geometry contracts over real DOM receivers and placed text.
use serde_json::{json, Value};
use turbo_surf_render::run_with_dom;

fn evaluate(html: &str, source: &str) -> Value {
    serde_json::from_str(&run_with_dom(html, source).unwrap()).unwrap()
}

#[test]
fn inline_text_scrolling_uses_placed_runs_and_hidden_text_does_not_move_the_port() {
    let actual = evaluate(
        r#"<!doctype html><style>
        html,body{margin:0}#port{width:180px;height:70px;overflow:auto}
        #space{height:360px}#line{white-space:nowrap;width:600px;font:16px/24px monospace}
        #hidden{display:none}
        </style><div id="port"><div id="space"></div><div id="line"><span id="target">placed text</span><span id="hidden">hidden text</span></div></div>"#,
        r#"JSON.stringify((()=>{
          const port=document.getElementById('port'),target=document.getElementById('target');
          const before=document.body.innerHTML;
          target.scrollIntoView({block:'nearest',inline:'nearest'});
          const p=port.getBoundingClientRect(),r=target.getBoundingClientRect();
          const visible=port.scrollTop>0&&r.top>=p.top&&r.bottom<=p.bottom;
          port.scrollTop=0;
          document.getElementById('hidden').scrollIntoView({block:'nearest',inline:'nearest'});
          return [visible,port.scrollTop,port.scrollLeft,target.textContent,
            document.body.innerHTML===before,target.getClientRects().length>0];
        })())"#,
    );
    assert_eq!(actual, json!([true, 0, 0, "placed text", true, true]));
}

#[test]
fn native_geometry_brands_require_actual_html_and_input_receivers() {
    let actual = evaluate(
        "<!doctype html><div id=box style='width:43px;height:17px'></div><input id=field type=text><svg id=vector></svg>",
        r#"JSON.stringify((()=>{
          const box=document.getElementById('box'),field=document.getElementById('field');
          const foreign=document.createElementNS('http://www.w3.org/2000/svg','input');
          document.getElementById('vector').append(foreign);
          const offset=Object.getOwnPropertyDescriptor(HTMLElement.prototype,'offsetWidth').get;
          const width=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'width');
          const deny=run=>{try{run();return 'accepted'}catch(error){return error.name}};
          const denied=[];
          for(const receiver of [null,7,{},Object.create(HTMLElement.prototype),foreign])
            denied.push(deny(()=>offset.call(receiver)));
          for(const receiver of [null,7,{},box,foreign]){
            denied.push(deny(()=>width.get.call(receiver)));
            denied.push(deny(()=>width.set.call(receiver,23)));
          }
          width.set.call(field,23);
          return {denied,width:offset.call(box),height:box.offsetHeight,
            reflected:field.getAttribute('width'),textInputWidth:width.get.call(field),
            foreignUntouched:foreign.getAttribute('width')===null,foreignNamespace:foreign.namespaceURI,
            healthy:box.textContent===''};
        })())"#,
    );
    assert_eq!(
        actual,
        json!({"denied":vec!["TypeError";15],"width":43,"height":17,
            "reflected":"23","textInputWidth":0,"foreignUntouched":true,"foreignNamespace":"http://www.w3.org/2000/svg","healthy":true})
    );
}

#[test]
fn svg_text_ranges_report_missing_native_geometry_without_poisoning_html_ranges() {
    let actual = evaluate(
        "<!doctype html><svg width=120 height=40><text id=vector x=0 y=20>vector</text></svg><p id=html>ordinary text</p>",
        r#"JSON.stringify((()=>{
          const before=document.body.innerHTML,vector=document.createRange();
          vector.selectNodeContents(document.getElementById('vector'));
          let diagnostic='silently admitted';
          try{vector.getClientRects()}catch(error){diagnostic=error.message}
          const html=document.createRange();html.selectNodeContents(document.getElementById('html'));
          const rect=html.getBoundingClientRect();
          return [diagnostic,html.getClientRects().length>0,rect.width>0,rect.height>0,
            document.body.innerHTML===before,document.getElementById('vector').textContent];
        })())"#,
    );
    assert_eq!(
        actual,
        json!([
            "native SVG text geometry is unsupported",
            true,
            true,
            true,
            true,
            "vector"
        ])
    );
}
