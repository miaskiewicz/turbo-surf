use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::{BrowserStorage, DocumentResponseMetadata, PageSession};

const FIXTURE: &str =
    include_str!("../../../../harness/detection/fixtures/activation-headers-chrome156.json");
const COLLECTOR: &str = include_str!("../../../../harness/detection/activation-headers-chrome.mjs");
const READ: &str = "({isActive:navigator.userActivation.isActive,hasBeenActive:navigator.userActivation.hasBeenActive})";

async fn read(page: &mut PageSession, expression: &str) -> Value {
    serde_json::from_str(&page.eval_json_expression(expression).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn public_activation_has_chrome_brands_and_native_context_state() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(
        fixture["sourceSha256"],
        format!("{:x}", Sha256::digest(COLLECTOR.as_bytes()))
    );
    let probe = COLLECTOR
        .split_once("activationShape = await evaluate(`")
        .unwrap()
        .1
        .split_once("`);")
        .unwrap()
        .0;
    assert_eq!(fixture["errors"], json!([]));
    let mut page = PageSession::open(
        "<button id=activate type=button>activate</button>",
        "https://activation.test/start",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let expected_shape = fixture["activationShape"].clone();
    assert_eq!(read(&mut page, probe).await, expected_shape);
    assert_eq!(
        read(&mut page, READ).await,
        fixture["results"][0]["initialActivation"]
    );
    assert_eq!(read(&mut page,"(()=>{const value=navigator.userActivation;const getter=Object.getOwnPropertyDescriptor(UserActivation.prototype,'isActive').get;Object.defineProperty(navigator,'userActivation',{value:{isActive:true,hasBeenActive:true},configurable:true});let error;try{getter.call(navigator.userActivation)}catch(value){error=value.name}const result=[getter.call(value),error];delete navigator.userActivation;return result})()").await,json!([false,"TypeError"]));
    page.input(&json!([{"click":"#activate"}])).await.unwrap();
    assert_eq!(
        read(&mut page, READ).await,
        fixture["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "trusted-click-location")
            .unwrap()["requests"][0]["activation"]
    );
    let snapshot = page.navigation_activation_snapshot();
    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    assert_eq!(
        read(&mut page, READ).await,
        fixture["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "expired-activation")
            .unwrap()["expiredActivation"]
    );
    for (destination, expected) in [
        (
            "https://activation.test/next",
            json!({"isActive":false,"hasBeenActive":true}),
        ),
        (
            "https://other.test/next",
            json!({"isActive":false,"hasBeenActive":false}),
        ),
    ] {
        let mut next=PageSession::open_with_document_response_and_activation("<script>globalThis.initialActivation={isActive:navigator.userActivation.isActive,hasBeenActive:navigator.userActivation.hasBeenActive}</script>",destination,"","",10000,"",Default::default(),&[],BrowserStorage::default(),DocumentResponseMetadata::default(),Some(snapshot.clone())).await.unwrap();
        assert_eq!(read(&mut next, "initialActivation").await, expected);
        next.close();
    }
    let mut fresh = PageSession::open("<body>", "https://activation.test/fresh", "", "", 10000)
        .await
        .unwrap();
    assert_eq!(
        read(&mut fresh, READ).await,
        json!({"isActive":false,"hasBeenActive":false})
    );
    fresh.close();
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_input_activates_its_native_document_tree_without_activating_a_peer_page() {
    let mut page = PageSession::open(
        "<button id=activate>activate</button><iframe id=child srcdoc='<p>child</p>'></iframe>",
        "https://activation.test/start",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    page.eval_expression("globalThis.peer=window.open('', 'activation-peer');void 0")
        .await
        .unwrap();
    const STATES: &str = "[window,document.getElementById('child').contentWindow,peer].map(w=>[w.navigator.userActivation.isActive,w.navigator.userActivation.hasBeenActive])";
    assert_eq!(
        read(&mut page, STATES).await,
        json!([[false, false], [false, false], [false, false]])
    );
    page.eval_expression("document.getElementById('activate').click();void 0")
        .await
        .unwrap();
    assert_eq!(
        read(&mut page, STATES).await,
        json!([[false, false], [false, false], [false, false]])
    );
    page.input(&json!([{"click":"#activate"}])).await.unwrap();
    assert_eq!(
        read(&mut page, STATES).await,
        json!([[true, true], [true, true], [false, false]])
    );
    let snapshot = page.navigation_activation_snapshot();
    let mut next = PageSession::open_with_document_response_and_activation(
        "<body>next",
        "https://activation.test/next",
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        Some(snapshot),
    )
    .await
    .unwrap();
    assert_eq!(
        read(&mut next, READ).await,
        json!({"isActive":false,"hasBeenActive":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_escape_closes_the_actual_input_document_dialog() {
    let mut page = PageSession::open(
        "<dialog id=box><button>inside</button></dialog><script>globalThis.closeEvents=0;globalThis.cancelEvents=0;const dialog=document.getElementById('box');dialog.addEventListener('close',()=>closeEvents++);dialog.addEventListener('cancel',()=>cancelEvents++);dialog.showModal();</script>",
        "https://activation.test/dialog", "", "", 60_000,
    ).await.unwrap();
    assert_eq!(
        read(&mut page, "document.getElementById('box').open").await,
        json!(true)
    );
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        read(
            &mut page,
            "[document.getElementById('box').open,cancelEvents,closeEvents]"
        )
        .await,
        json!([false, 1, 1])
    );
}
