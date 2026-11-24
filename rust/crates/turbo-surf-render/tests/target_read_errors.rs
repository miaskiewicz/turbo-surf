#[path = "support/popup_contract.rs"]
pub mod fixture;

#[tokio::test(flavor = "current_thread")]
async fn native_target_reads_preserve_root_and_peer_guest_exceptions() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    for target in [root, peer] {
        let error = page
            .read_target_engine(target, "throw new RangeError('layout-cause-marker')")
            .unwrap_err();
        assert!(error.contains("RangeError: layout-cause-marker"), "{error}");
        assert_eq!(
            page.read_target_engine(target, "return 21*2").unwrap(),
            "42"
        );
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn native_target_read_serialization_preserves_the_throwing_getter() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = page.auxiliary_root_id().unwrap();
    let error = page
        .read_target_engine(
            target,
            "return {get value(){throw new TypeError('serialization-cause-marker')}}",
        )
        .unwrap_err();
    assert!(
        error.contains("TypeError: serialization-cause-marker"),
        "{error}"
    );
    assert_eq!(page.read_target_engine(target, "return 42").unwrap(), "42");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn native_target_error_formatting_retains_the_execution_watchdog() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = page.auxiliary_root_id().unwrap();
    let started = std::time::Instant::now();
    let error = page
        .read_target_engine(target, "throw {toString(){while(true){}}}")
        .unwrap_err();
    assert!(error.contains("session budget"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(page.read_target_engine(target, "return 42").unwrap(), "42");
    page.close();
}
