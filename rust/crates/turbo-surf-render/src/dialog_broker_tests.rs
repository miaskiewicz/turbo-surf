use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

fn request(owner: &str) -> DialogRequest {
    DialogRequest {
        id: 0,
        owner: owner.into(),
        primary: owner == "root",
        kind: DialogKind::Prompt,
        message: "message".encode_utf16().collect(),
        default_value: "initial".encode_utf16().collect(),
    }
}
#[test]
fn primary_listener_is_available_before_first_script_but_does_not_leak_to_children() {
    let broker = DialogBroker::default();
    broker.configure_primary(true).unwrap();
    let (event, done) = waiting(&broker, "root");
    broker
        .respond("root", event.id, DialogReply::dismissed())
        .unwrap();
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
    let child = broker
        .request(
            request("child"),
            Instant::now(),
            || false,
            |_| panic!("opener Page listener must not enable child dialogs"),
        )
        .unwrap();
    assert_eq!(child, DialogReply::dismissed());
}
#[test]
fn context_listener_is_inherited_by_future_child_targets() {
    let broker = DialogBroker::default();
    broker.configure_context(true).unwrap();
    let (event, done) = waiting(&broker, "child");
    broker
        .respond("child", event.id, DialogReply::dismissed())
        .unwrap();
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
    broker.configure_context(false).unwrap();
    assert_eq!(
        broker
            .request(
                request("later-child"),
                Instant::now(),
                || false,
                |_| panic!("removed context listener must not enable a later child")
            )
            .unwrap(),
        DialogReply::dismissed()
    );
}
#[test]
fn a_prepublication_primary_reply_cannot_answer_a_child_request() {
    let broker = DialogBroker::default();
    broker.configure_context(true).unwrap();
    let (event, done) = waiting(&broker, "child");
    assert!(broker
        .respond_primary(event.id, DialogReply::dismissed())
        .is_err());
    assert_eq!(done.try_recv(), Err(mpsc::TryRecvError::Empty));
    broker
        .respond("child", event.id, DialogReply::dismissed())
        .unwrap();
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
}
#[test]
fn cancelling_initial_root_releases_its_dialog_and_preserves_child_policy() {
    let broker = DialogBroker::default();
    broker.configure_context(true).unwrap();
    let (_, done) = waiting(&broker, "root");
    broker.cancel_primary();
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    assert!(broker
        .request(request("root"), Instant::now(), || false, |_| {})
        .is_err());
    let (event, child) = waiting(&broker, "child");
    broker
        .respond("child", event.id, DialogReply::dismissed())
        .unwrap();
    assert!(child.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
}
fn waiting(
    broker: &DialogBroker,
    owner: &str,
) -> (DialogRequest, mpsc::Receiver<Result<DialogReply, String>>) {
    let broker = broker.clone();
    let request = request(owner);
    let (events, event_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = broker.request(
            request,
            Instant::now() + Duration::from_secs(1),
            || false,
            |event| events.send(event).unwrap(),
        );
        done.send(result).unwrap();
    });
    (
        event_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("native dialog must reach host"),
        done_rx,
    )
}
#[test]
fn no_listener_dismisses_without_publishing_or_blocking() {
    let broker = DialogBroker::default();
    let reply = broker
        .request(
            request("root"),
            Instant::now(),
            || false,
            |_| panic!("no-listener dialog must auto-dismiss"),
        )
        .unwrap();
    assert_eq!(reply, DialogReply::dismissed());
}
#[test]
fn host_reply_resumes_exact_pending_dialog_once() {
    let broker = DialogBroker::default();
    broker.configure("root", true).unwrap();
    let (event, done) = waiting(&broker, "root");
    assert_eq!(event.kind, DialogKind::Prompt);
    assert_eq!(event.message, "message".encode_utf16().collect::<Vec<_>>());
    assert_eq!(
        event.default_value,
        "initial".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(done.try_recv(), Err(mpsc::TryRecvError::Empty));
    let reply = DialogReply {
        accepted: true,
        prompt_text: Some("accepted-value".encode_utf16().collect()),
    };
    broker.respond("root", event.id, reply.clone()).unwrap();
    assert_eq!(
        done.recv_timeout(Duration::from_secs(1)).unwrap().unwrap(),
        reply
    );
    assert!(broker
        .respond("root", event.id, DialogReply::dismissed())
        .is_err());
}
#[test]
fn another_target_cannot_answer_the_dialog() {
    let broker = DialogBroker::default();
    broker.configure("child", true).unwrap();
    let (event, done) = waiting(&broker, "child");
    assert!(broker
        .respond("root", event.id, DialogReply::dismissed())
        .is_err());
    assert_eq!(done.try_recv(), Err(mpsc::TryRecvError::Empty));
    broker
        .respond("child", event.id, DialogReply::dismissed())
        .unwrap();
    assert_eq!(
        done.recv_timeout(Duration::from_secs(1)).unwrap().unwrap(),
        DialogReply::dismissed()
    );
}
#[test]
fn retiring_one_target_wakes_only_its_dialog() {
    let broker = DialogBroker::default();
    broker.configure("child", true).unwrap();
    broker.configure("other", true).unwrap();
    let (_, child) = waiting(&broker, "child");
    let (other_event, other) = waiting(&broker, "other");
    broker.cancel_owner("child");
    assert!(child
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err()
        .contains("closed"));
    assert_eq!(other.try_recv(), Err(mpsc::TryRecvError::Empty));
    broker
        .respond("other", other_event.id, DialogReply::dismissed())
        .unwrap();
    assert!(other.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
}
#[test]
fn closing_the_actor_wakes_every_dialog_and_rejects_new_ones() {
    let broker = DialogBroker::default();
    broker.configure("root", true).unwrap();
    let (_, done) = waiting(&broker, "root");
    broker.close();
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    assert!(broker
        .request(request("root"), Instant::now(), || false, |_| {})
        .is_err());
}
#[test]
fn deadline_and_execution_cancellation_remove_pending_replies() {
    let broker = DialogBroker::default();
    broker.configure("root", true).unwrap();
    let mut event = None;
    let result = broker.request(
        request("root"),
        Instant::now() + Duration::from_millis(5),
        || false,
        |request| event = Some(request),
    );
    assert!(result.unwrap_err().contains("timeout"));
    assert!(broker
        .respond("root", event.unwrap().id, DialogReply::dismissed())
        .is_err());
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    let result = broker.request(
        request("root"),
        Instant::now() + Duration::from_secs(1),
        || cancelled.load(Ordering::Acquire),
        |_| {
            cancel.store(true, Ordering::Release);
        },
    );
    assert!(result.unwrap_err().contains("cancelled"));
}
