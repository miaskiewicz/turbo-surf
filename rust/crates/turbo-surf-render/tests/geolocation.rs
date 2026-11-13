use serde_json::{json, Value};
use turbo_surf_render::{BrowserStorage, PageSession};

async fn page(storage: BrowserStorage, origin: &str, headers: &[(String, String)]) -> PageSession {
    PageSession::open_with_browsing_state(
        "<!doctype html><body></body>",
        origin,
        "",
        "",
        3000,
        "",
        Default::default(),
        headers,
        storage,
    )
    .await
    .unwrap()
}
async fn value(page: &mut PageSession, expression: &str) -> Value {
    serde_json::from_str(&page.eval_json_expression(expression).await.unwrap()).unwrap()
}
const POSITION: &str = r#"new Promise(resolve => {
    let synchronous = true;
    const timer = setTimeout(() => resolve({missing:true}), 100);
    navigator.geolocation.getCurrentPosition(position => {
        clearTimeout(timer);
        resolve({latitude:position.coords.latitude, longitude:position.coords.longitude,
            accuracy:position.coords.accuracy, synchronous});
    }, error => {clearTimeout(timer);resolve({code:error.code,synchronous});});
    synchronous = false;
})"#;

#[tokio::test(flavor = "current_thread")]
async fn configured_grant_delivers_real_coordinates_asynchronously() {
    let storage = BrowserStorage::default();
    storage.configure_emulation(r#"{"permissions":[{"permissions":["geolocation"]}],"geolocation":{"latitude":40,"longitude":-74}}"#).unwrap();
    let mut page = page(storage, "http://localhost/", &[]).await;
    assert_eq!(
        value(
            &mut page,
            "navigator.permissions.query({name:'geolocation'}).then(p=>p.state)"
        )
        .await,
        "granted"
    );
    assert_eq!(
        value(&mut page, POSITION).await,
        json!({"latitude":40,"longitude":-74,"accuracy":0,"synchronous":false})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn origin_grants_are_isolated_clearable_and_denied_by_policy() {
    let storage = BrowserStorage::default();
    storage.configure_emulation(r#"{"permissions":[{"origin":"http://localhost","permissions":["geolocation"]}],"geolocation":{"latitude":40,"longitude":-74}}"#).unwrap();
    let mut allowed = page(storage.clone(), "http://localhost/", &[]).await;
    let mut other = page(storage.clone(), "https://other.test/", &[]).await;
    let query = "navigator.permissions.query({name:'geolocation'}).then(p=>p.state)";
    assert_eq!(value(&mut allowed, query).await, "granted");
    assert_eq!(value(&mut other, query).await, "prompt");
    let mut denied = page(
        storage.clone(),
        "http://localhost/",
        &[("permissions-policy".into(), "geolocation=()".into())],
    )
    .await;
    assert_eq!(value(&mut denied, query).await, "denied");
    assert_eq!(value(&mut denied, POSITION).await["code"], 1);
    storage.configure_emulation("{}").unwrap();
    assert_eq!(value(&mut allowed, query).await, "prompt");
}

#[tokio::test(flavor = "current_thread")]
async fn watch_receives_native_updates_and_clear_watch_stops_delivery() {
    let storage = BrowserStorage::default();
    let configure = |latitude| {
        format!(
            r#"{{"permissions":[{{"permissions":["geolocation"]}}],"geolocation":{{"latitude":{latitude},"longitude":2}}}}"#
        )
    };
    storage.configure_emulation(&configure(1)).unwrap();
    let mut page = page(storage.clone(), "http://localhost/", &[]).await;
    assert_eq!(value(&mut page, "new Promise(resolve=>{globalThis.positions=[];globalThis.watch=navigator.geolocation.watchPosition(p=>{positions.push(p.coords.latitude);resolve(positions.length)});})").await, 1);
    storage.configure_emulation(&configure(3)).unwrap();
    assert_eq!(value(&mut page, "new Promise(resolve=>{const check=()=>positions.length===2?resolve(positions):setTimeout(check,1);check();})").await, json!([1,3]));
    value(&mut page, "(navigator.geolocation.clearWatch(watch), true)").await;
    storage.configure_emulation(&configure(4)).unwrap();
    assert_eq!(
        value(
            &mut page,
            "new Promise(resolve=>setTimeout(()=>resolve(positions),20))"
        )
        .await,
        json!([1, 3])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn missing_position_and_revoked_permission_deliver_errors() {
    let storage = BrowserStorage::default();
    storage
        .configure_emulation(r#"{"permissions":[{"permissions":["geolocation"]}]}"#)
        .unwrap();
    let mut page = page(storage.clone(), "http://localhost/", &[]).await;
    assert_eq!(
        value(&mut page, POSITION).await,
        json!({"code":2,"synchronous":false})
    );
    storage.configure_emulation("{}").unwrap();
    assert_eq!(
        value(&mut page, POSITION).await,
        json!({"code":1,"synchronous":false})
    );
}

#[test]
fn invalid_emulation_never_replaces_previous_config() {
    let storage = BrowserStorage::default();
    for options in [
        r#"{"geolocation":{"latitude":91,"longitude":0}}"#,
        r#"{"geolocation":{"latitude":0,"longitude":181}}"#,
        r#"{"geolocation":{"latitude":0,"longitude":0,"accuracy":-1}}"#,
        r#"{"permissions":[{"origin":"not a URL","permissions":["geolocation"]}]}"#,
    ] {
        assert!(storage.configure_emulation(options).is_err(), "{options}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_permission_status_changes_and_child_frames_use_their_real_document_policy() {
    let storage = BrowserStorage::default();
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><iframe srcdoc='<body>child</body>'></iframe>",
        "http://localhost/",
        "",
        "",
        3000,
        "",
        Default::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    assert_eq!(value(&mut page, "(async()=>{globalThis.geoStatus=await navigator.permissions.query({name:'geolocation'});globalThis.geoChanges=[];geoStatus.onchange=()=>geoChanges.push(geoStatus.state);return geoStatus.state;})()").await, "prompt");
    storage.configure_emulation(r#"{"permissions":[{"permissions":["geolocation"]}],"geolocation":{"latitude":7,"longitude":8}}"#).unwrap();
    assert_eq!(value(&mut page, "new Promise(resolve=>{const check=()=>geoChanges.length?resolve(geoChanges):setTimeout(check,1);check();})").await, json!(["granted"]));
    assert_eq!(value(&mut page, "new Promise(resolve=>document.querySelector('iframe').contentWindow.navigator.geolocation.getCurrentPosition(p=>resolve([p.coords.latitude,p.coords.longitude]),e=>resolve({code:e.code})))").await, json!([7,8]));
    page.eval(r#"document.querySelector('iframe').allow="geolocation 'none'";"#)
        .await
        .unwrap();
    assert_eq!(value(&mut page, "document.querySelector('iframe').contentWindow.navigator.permissions.query({name:'geolocation'}).then(p=>p.state)").await, "denied");
}

#[tokio::test(flavor = "current_thread")]
async fn revocation_before_a_queued_position_callback_never_discloses_coordinates() {
    let storage = BrowserStorage::default();
    storage.configure_emulation(r#"{"permissions":[{"permissions":["geolocation"]}],"geolocation":{"latitude":40,"longitude":-74}}"#).unwrap();
    let mut page = page(storage.clone(), "http://localhost/", &[]).await;
    page.read_engine("globalThis.lateGeo=null;navigator.geolocation.getCurrentPosition(p=>{lateGeo={latitude:p.coords.latitude};},e=>{lateGeo={code:e.code};});return true;").unwrap();
    storage.configure_emulation("{}").unwrap();
    assert_eq!(
        value(
            &mut page,
            "new Promise(resolve=>setTimeout(()=>resolve(lateGeo),1))"
        )
        .await,
        json!({"code":1})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn watch_sampling_does_not_invoke_guest_json_hooks() {
    let storage = BrowserStorage::default();
    let configure = |latitude| {
        format!(
            r#"{{"permissions":[{{"permissions":["geolocation"]}}],"geolocation":{{"latitude":{latitude},"longitude":2}}}}"#
        )
    };
    storage.configure_emulation(&configure(1)).unwrap();
    let mut page = page(storage.clone(), "http://localhost/", &[]).await;
    assert_eq!(value(&mut page, "new Promise(resolve=>{globalThis.samples=[];globalThis.watch=navigator.geolocation.watchPosition(p=>{samples.push(p.coords.latitude);delete Object.prototype.toJSON;resolve(samples.length)});})").await, 1);
    page.read_engine("Object.prototype.toJSON=()=>{throw new Error('hostile native-sample toJSON')};return true;").unwrap();
    storage.configure_emulation(&configure(3)).unwrap();
    assert_eq!(value(&mut page, "new Promise(resolve=>{const check=()=>samples.length===2?resolve(samples):setTimeout(check,1);check();})").await, json!([1,3]));
}

#[tokio::test(flavor = "current_thread")]
async fn pending_watch_wakes_when_host_coordinates_change_without_polling_timers() {
    use std::future::Future;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::{Context, Wake, Waker};
    #[derive(Default)]
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let storage = BrowserStorage::default();
    let configure = |latitude| {
        format!(
            r#"{{"permissions":[{{"permissions":["geolocation"]}}],"geolocation":{{"latitude":{latitude},"longitude":2}}}}"#
        )
    };
    storage.configure_emulation(&configure(1)).unwrap();
    let mut page = page(storage.clone(), "http://localhost/", &[]).await;
    page.eval("globalThis.updated=new Promise(resolve=>{globalThis.resolvePosition=resolve});navigator.geolocation.watchPosition(p=>{if(p.coords.latitude===3)resolvePosition(p.coords.latitude)});").await.unwrap();
    let counter = Arc::new(Counter::default());
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    let future = page.eval_json_expression("updated");
    let mut future = std::pin::pin!(future);
    assert!(
        future.as_mut().poll(&mut cx).is_pending(),
        "position update must remain pending until the host changes it"
    );
    counter.0.store(0, Ordering::SeqCst);
    storage.configure_emulation(&configure(3)).unwrap();
    assert!(
        counter.0.load(Ordering::SeqCst) > 0,
        "host configuration must wake the pending native operation"
    );
    assert_eq!(future.await.unwrap(), "3");
}

#[tokio::test(flavor = "current_thread")]
async fn removed_frame_watches_cannot_deliver_later_coordinates() {
    let storage = BrowserStorage::default();
    let configure = |latitude| {
        format!(
            r#"{{"permissions":[{{"permissions":["geolocation"]}}],"geolocation":{{"latitude":{latitude},"longitude":2}}}}"#
        )
    };
    storage.configure_emulation(&configure(1)).unwrap();
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><iframe srcdoc='<body>child</body>'></iframe>",
        "http://localhost/",
        "",
        "",
        3000,
        "",
        Default::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    assert_eq!(value(&mut page, "new Promise(resolve=>{globalThis.frameSamples=[];document.querySelector('iframe').contentWindow.navigator.geolocation.watchPosition(p=>{frameSamples.push(p.coords.latitude);resolve(frameSamples)});})").await, json!([1]));
    page.read_engine("document.querySelector('iframe').remove();return true;")
        .unwrap();
    storage.configure_emulation(&configure(3)).unwrap();
    assert_eq!(
        value(
            &mut page,
            "new Promise(resolve=>setTimeout(()=>resolve(frameSamples),1))"
        )
        .await,
        json!([1])
    );
}
