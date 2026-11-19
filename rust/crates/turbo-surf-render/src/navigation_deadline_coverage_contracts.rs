use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<title>healthy</title>",
        "https://deadline.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn obsolete_driver_guard_cannot_clear_its_replacement_lease() {
    let mut page = page().await;
    let target = page.auxiliary_root_id().unwrap();
    // Generations are host lease identifiers, not document IDs or guest authority.
    let old = DriverDeadline::start(&mut page.rt, 5000, 71, target, ExecutionGoal::Complete);
    let current = DriverDeadline::start(&mut page.rt, 5000, 72, target, ExecutionGoal::Complete);
    assert!(old.unfinished_frames().frames.is_empty());
    drop(old);
    {
        let state = page.rt.op_state();
        let state = state.borrow();
        let active = state.borrow::<Active>();
        assert!(owns_active(active, 72, &current.clock));
        assert_eq!(active.target, target);
        assert!(!current.exceeded());
    }
    drop(current);
    assert!(!expired(&page.rt));
    assert_eq!(
        page.eval_json_expression("[document.title,location.href]")
            .await
            .unwrap(),
        "[\"healthy\",\"https://deadline.test/\"]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_setup_expiry_rejects_reentry_without_terminating_the_root() {
    let mut page = page().await;
    let target = page.auxiliary_root_id().unwrap();
    let deadline = DriverDeadline::start(&mut page.rt, 500, 81, target, ExecutionGoal::Complete);
    let setup = TrustedSetup::enter(&mut page.rt, target, 81).unwrap();
    let expires = {
        let clock = deadline.clock.lock().unwrap();
        clock.started.checked_add(clock.budget).unwrap()
    };
    loop {
        let remaining = expires.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining);
    }
    assert!(!page.rt.v8_isolate().is_execution_terminating());
    assert_eq!(
        setup.finish().unwrap_err(),
        "Native target navigation exceeded session budget"
    );
    assert_eq!(
        TrustedSetup::enter(&mut page.rt, target, 81).err().unwrap(),
        "Native target navigation exceeded session budget"
    );
    assert!(deadline.exceeded());
    drop(deadline);
    let recovery = DriverDeadline::start(&mut page.rt, 5000, 82, target, ExecutionGoal::Complete);
    TrustedSetup::enter(&mut page.rt, target, 82)
        .unwrap()
        .finish()
        .unwrap();
    assert!(!recovery.exceeded());
    drop(recovery);
    assert_eq!(
        page.eval_json_expression("[document.title,1+1]")
            .await
            .unwrap(),
        "[\"healthy\",2]"
    );
}
