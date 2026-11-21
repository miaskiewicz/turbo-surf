//! Exploratory control for unrelated actions beside an abandoned pending parent.
//! This does not establish the cause of the application auth failure.
use turbo_surf_render::PageSession;

const HTML: &str = "<!doctype html><button id='open'>Open</button><div id='modal' hidden>Dialog</div><script>globalThis.clicks=0;document.querySelector('#open').onclick=()=>{clicks++;document.querySelector('#modal').hidden=false;};</script>";
const ACTION: &str = "document.querySelector('#open').click();globalThis.__RESULT='clicked';";

async fn fixture(retain_parent: bool) -> (PageSession, u64) {
    let mut page = PageSession::open(HTML, "https://pending-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let retain = if retain_parent {
        "globalThis.retainedParent=parent;"
    } else {
        ""
    };
    let source = format!(
        "const parent=new Promise(()=>{{}});globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(()=>{{globalThis.unexpected=true;}});{retain}return 'armed';"
    );
    assert_eq!(page.read_target_engine(root, &source).unwrap(), "\"armed\"");
    (page, root)
}

fn modal_state(page: &mut PageSession, root: u64) -> String {
    page.read_target_engine(
        root,
        "return [clicks,document.querySelector('#modal').hidden,globalThis.unexpected===undefined];",
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn collected_pending_parent_does_not_poison_unrelated_modal_action() {
    let (mut page, root) = fixture(false).await;
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap()
            == "true"
        {
            break;
        }
    }
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap(),
        "true",
        "the actual native parent must have been collected"
    );
    assert_eq!(modal_state(&mut page, root), "[0,true,true]");
    let action = page.dispatch_target_engine(root, ACTION).await;
    assert_eq!(
        modal_state(&mut page, root),
        "[1,false,true]",
        "the actual guest click opens its modal exactly once"
    );
    let pending = page
        .eval_json_expression("observedChild")
        .await
        .unwrap_err();
    assert!(
        pending.contains("Native async work has unclassified ownership"),
        "a collected pending parent cannot become fulfilled: {pending}"
    );
    assert_eq!(modal_state(&mut page, root), "[1,false,true]");
    assert_eq!(action.unwrap(), "clicked");
}

#[tokio::test(flavor = "current_thread")]
async fn live_pending_parent_permits_unrelated_modal_action() {
    let (mut page, root) = fixture(true).await;
    page.collect_message_port_garbage_for_testing();
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===retainedParent;")
            .unwrap(),
        "true"
    );
    assert_eq!(modal_state(&mut page, root), "[0,true,true]");
    assert_eq!(
        page.dispatch_target_engine(root, ACTION).await.unwrap(),
        "clicked"
    );
    assert_eq!(modal_state(&mut page, root), "[1,false,true]");
}

#[tokio::test(flavor = "current_thread")]
async fn collected_foreign_pending_parent_is_not_authorized_by_the_root() {
    let mut page = PageSession::open(HTML, "https://pending-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression(
        "globalThis.peerWindow=window.open('about:blank','foreign-parent');void 0",
    )
    .await
    .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    page.read_target_engine(
        peer,
        "globalThis.pendingParent=new Promise(()=>{});return 'armed';",
    )
    .unwrap();
    page.read_target_engine(root, "const parent=peerWindow.pendingParent;globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(()=>{globalThis.unexpected=true;});peerWindow.pendingParent=null;return 'attached';")
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap()
            == "true"
        {
            break;
        }
    }
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap(),
        "true"
    );
    let error = page.dispatch_target_engine(root, ACTION).await.unwrap_err();
    assert!(
        error.contains("Native async work has unclassified ownership"),
        "a foreign parent cannot acquire Root authority through collection: {error}"
    );
    assert_eq!(modal_state(&mut page, root), "[1,false,true]");
}

#[tokio::test(flavor = "current_thread")]
async fn collected_root_pending_parent_does_not_poison_an_independent_timer_turn() {
    let (mut page, root) = fixture(false).await;
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap()
            == "true"
        {
            break;
        }
    }
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap(),
        "true"
    );
    page.read_target_engine(root, "globalThis.timerRuns=0;setTimeout(()=>{timerRuns++;document.querySelector('#modal').hidden=false;},1);return 'armed';")
        .unwrap();
    assert_eq!(
        page.eval_json_expression("new Promise(resolve=>setTimeout(()=>resolve(timerRuns),2))")
            .await
            .unwrap(),
        "1"
    );
    assert_eq!(
        page.read_target_engine(root, "return [timerRuns,document.querySelector('#modal').hidden,globalThis.unexpected===undefined];")
            .unwrap(),
        "[1,false,true]"
    );
}
