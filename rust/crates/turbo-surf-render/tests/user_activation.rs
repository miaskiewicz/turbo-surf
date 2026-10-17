use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::{BrowserStorage, DocumentResponseMetadata, PageSession};

const FIXTURE: &str =
    include_str!("../../../../harness/detection/fixtures/activation-headers-chrome155.json");
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
