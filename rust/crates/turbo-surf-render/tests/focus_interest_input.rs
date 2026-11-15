//! Trusted host input drives the binding's focusgroup and interest-invoker default actions:
//! arrow keys move focus within a focusgroup, and hover/focus show an `interestfor` popover
//! after its CSS-timed delay on the virtual timers. Guest-dispatched events never do.
use serde_json::json;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn trusted_arrow_keys_move_focus_within_a_focusgroup() {
    let mut page = PageSession::open(
        "<body><div focusgroup=toolbar><button id=a>a</button><button id=b disabled>b</button><button id=c>c</button><button id=d>d</button></div><input id=after></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("globalThis.seen=[];for(const type of ['keydown','focus','blur'])document.addEventListener(type,event=>seen.push([type,event.target.id,event.isTrusted]),true);")
        .await
        .unwrap();
    let active = "document.activeElement.id";
    page.input(&json!([{"focus":"#a"},{"press":"ArrowRight"}]))
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"c\"");
    assert_eq!(
        page.eval_json_expression("seen.slice(1)").await.unwrap(),
        "[[\"keydown\",\"a\",true],[\"blur\",\"a\",true],[\"focus\",\"c\",true]]"
    );
    // toolbar: inline axis only, no wrapping; End/Home jump to the ends.
    page.input(&json!([{"press":"ArrowDown"},{"press":"End"}]))
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"d\"");
    page.input(&json!([{"press":"ArrowRight"}])).await.unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"d\"");
    page.input(&json!([{"press":"Home"}])).await.unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"a\"");
    // A guest-dispatched keydown is untrusted and has no default action.
    page.eval("document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true,cancelable:true}))")
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"a\"");
    // A canceled trusted keydown keeps focus.
    page.eval("document.addEventListener('keydown',event=>event.preventDefault(),{once:true})")
        .await
        .unwrap();
    page.input(&json!([{"press":"ArrowRight"}])).await.unwrap();
    assert_eq!(page.eval_json_expression(active).await.unwrap(), "\"a\"");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_hover_and_focus_show_interest_targets_after_css_delays() {
    let mut page = PageSession::open(
        "<style>#slow{interest-delay:1s 400ms}</style><body><button id=b interestfor=p>b</button><button id=slow interestfor=p>slow</button><div id=p popover>pop</div><div id=park>park</div></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    // Every entry carries the timer clock, so delays are checked against the trigger's own
    // timestamp however long the host takes between input steps.
    page.eval("globalThis.log=[];const at=()=>performance.now();const p=document.querySelector('#p');for(const type of ['interest','loseinterest','beforetoggle','toggle'])p.addEventListener(type,event=>log.push([type,event.source&&event.source.id,event.isTrusted,at()]));for(const id of ['b','slow'])for(const type of ['mouseover','mouseout','focus','keydown'])document.querySelector('#'+id).addEventListener(type,event=>log.push([type,id,event.isTrusted,at()]));")
        .await
        .unwrap();
    let types = "log.map(entry=>entry[0]+':'+entry[1])";
    let delay = |from: &str, to: &str| {
        format!("(log.find(entry=>entry[0]==='{to}')[3]-log.find(entry=>entry[0]==='{from}')[3])")
    };
    page.input(&json!([{"move":{"selector":"#b"}},{"wait":600}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression(types).await.unwrap(),
        "[\"mouseover:b\",\"interest:b\",\"beforetoggle:b\",\"toggle:b\"]"
    );
    assert_eq!(
        page.eval_json_expression(&format!("[{}>=500,log.every(entry=>entry[2]),document.querySelector('#p').matches(':popover-open'),document.querySelector(':interest-source').id]", delay("mouseover", "interest")))
            .await
            .unwrap(),
        "[true,true,true,\"b\"]"
    );
    page.eval("log.length=0").await.unwrap();
    page.input(&json!([{"move":{"selector":"#park"}},{"wait":400}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression(types).await.unwrap(),
        "[\"mouseout:b\",\"loseinterest:b\",\"beforetoggle:b\",\"toggle:b\"]"
    );
    assert_eq!(
        page.eval_json_expression(&format!("[{}>=250,document.querySelector('#p').matches(':popover-open'),document.querySelectorAll(':interest-source').length]", delay("mouseout", "loseinterest")))
            .await
            .unwrap(),
        "[true,false,0]"
    );
    // Programmatic focus() honors the invoker's own interest-delay-start.
    page.eval("log.length=0;document.querySelector('#slow').focus()")
        .await
        .unwrap();
    page.input(&json!([{"wait":1200}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(&format!(
            "[{types},{}>=1000,document.querySelector('#p').matches(':popover-open')]",
            delay("focus", "interest")
        ))
        .await
        .unwrap(),
        "[[\"focus:slow\",\"interest:slow\",\"beforetoggle:slow\",\"toggle:slow\"],true,true]"
    );
    // A trusted Escape ends the interest at once, without the end delay.
    page.eval("log.length=0").await.unwrap();
    page.input(&json!([{"press":"Escape"},{"wait":50}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression(&format!("[{types},{}<100,document.querySelector('#p').matches(':popover-open')]", delay("keydown", "loseinterest")))
            .await
            .unwrap(),
        "[[\"keydown:slow\",\"loseinterest:slow\",\"beforetoggle:slow\",\"toggle:slow\"],true,false]"
    );
    // Guest-dispatched hover events never gain interest.
    page.eval("log.length=0;document.querySelector('#b').dispatchEvent(new MouseEvent('mouseover',{bubbles:true}))")
        .await
        .unwrap();
    page.input(&json!([{"wait":700}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(&format!(
            "[{types},document.querySelector('#p').matches(':popover-open')]"
        ))
        .await
        .unwrap(),
        "[[\"mouseover:b\"],false]"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_escape_keeps_its_private_document_and_leaves_peer_watchers_open() {
    let mut page = PageSession::open(
        "<dialog id=box><button id=inside>inside</button></dialog>",
        "https://escape-owner.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{
      globalThis.escapePeer=window.open('', 'escape-peer');
      const peerDialog=escapePeer.document.createElement('dialog');
      escapePeer.document.body.appendChild(peerDialog);peerDialog.showModal();
      globalThis.peerDialog=peerDialog;globalThis.closeCounts={root:0,peer:0};
      peerDialog.addEventListener('close',()=>closeCounts.peer++);
      document.getElementById('box').addEventListener('close',()=>closeCounts.root++);
      document.getElementById('box').showModal();document.getElementById('inside').focus();
      globalThis.__document=escapePeer.document;
      document.getElementById('inside').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
      return 'ready';
    })()"#).await.unwrap();
    let state =
        "[document.getElementById('box').open,peerDialog.open,closeCounts.root,closeCounts.peer]";
    assert_eq!(
        page.eval_json_expression(state).await.unwrap(),
        "[true,true,0,0]"
    );
    page.eval("document.getElementById('inside').addEventListener('keydown',event=>event.preventDefault(),{once:true})")
        .await
        .unwrap();
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(state).await.unwrap(),
        "[true,true,0,0]"
    );
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(state).await.unwrap(),
        "[false,true,1,0]"
    );
    // Neither a forged object nor a retained retired peer document replaces the
    // driver's captured document. Each trusted request still closes only root.
    page.eval("globalThis.__document={};document.getElementById('box').showModal();document.getElementById('inside').focus()")
        .await
        .unwrap();
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(state).await.unwrap(),
        "[false,true,2,0]"
    );
    page.eval("globalThis.__document=escapePeer.document;escapePeer.close();document.getElementById('box').showModal();document.getElementById('inside').focus()")
        .await
        .unwrap();
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression(
            "[document.getElementById('box').open,escapePeer.closed,closeCounts.root]"
        )
        .await
        .unwrap(),
        "[false,true,3]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_keyboard_dispatch_accepts_a_same_origin_adopted_target() {
    let mut page = PageSession::open("<body></body>", "https://adopted-input.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression(
        r#"(()=>{
      const peer=window.open('', 'adoption-peer');
      const button=peer.document.createElement('button');button.id='adopted';
      document.body.appendChild(document.adoptNode(button));
      globalThis.adoptedKeys=[];
      button.addEventListener('keydown',event=>adoptedKeys.push([event.key,event.isTrusted]));
      button.focus();
    })()"#,
    )
    .await
    .unwrap();
    page.input(&json!([{"press":"ArrowRight"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression("adoptedKeys").await.unwrap(),
        "[[\"ArrowRight\",true]]"
    );
}
