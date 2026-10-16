//! Native storage notices contain no VM handles or guest callbacks.
use std::sync::Arc;

#[derive(Debug)]
pub(crate) enum InboxError {
    Budget,
    Registration,
    Unavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Mutation {
    pub key: Option<Vec<u16>>,
    pub old: Option<Vec<u16>>,
    pub new: Option<Vec<u16>>,
    pub url: String,
}
pub(crate) struct Membership {
    pub(crate) local: u32,
    pub(crate) session: u32,
    pub(crate) active: std::sync::atomic::AtomicBool,
}
#[derive(Default)]
pub(crate) struct Inbox {
    pub(crate) areas: std::collections::VecDeque<Notice>,
    pub(crate) overflow: bool,
    pub(crate) registration: u64,
    pub(crate) memberships: Vec<std::sync::Weak<Membership>>,
    pub(crate) bytes: usize,
    waiter: Option<RegisteredWaiter>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Notice {
    pub(crate) area: u32,
    pub(crate) registration: u64,
    pub(crate) mutation: Option<Arc<Mutation>>,
}
impl Inbox {
    pub(crate) fn push(&mut self, area: u32) {
        if self.areas.len() == 8192 {
            self.overflow = true;
        } else {
            self.areas.push_back(Notice {
                area,
                registration: self.registration,
                mutation: None,
            });
        }
    }

    pub(crate) fn next(&mut self) -> Result<Option<Notice>, InboxError> {
        if std::mem::replace(&mut self.overflow, false) {
            return Err(InboxError::Budget);
        }
        Ok(self.areas.front().cloned())
    }

    pub(crate) fn interested(&mut self, area: u32) -> usize {
        self.memberships.retain(|entry| entry.strong_count() != 0);
        self.memberships
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .filter(|entry| {
                entry.active.load(std::sync::atomic::Ordering::Acquire)
                    && (entry.local == area || entry.session == area)
            })
            .count()
    }

    fn ensure_mutation_capacity(&self, bytes: usize, recipients: usize) -> Result<(), InboxError> {
        if self.areas.len() >= 8192
            || recipients > 8192
            || self.bytes.saturating_add(bytes) > 64 * 1024 * 1024
        {
            return Err(InboxError::Budget);
        }
        Ok(())
    }

    fn push_mutation(&mut self, area: u32, mutation: Arc<Mutation>) {
        self.bytes += payload_bytes(&mutation);
        self.areas.push_back(Notice {
            area,
            registration: self.registration,
            mutation: Some(mutation),
        });
    }

    pub(crate) fn pop(&mut self) {
        if let Some(notice) = self.areas.pop_front() {
            if let Some(mutation) = notice.mutation {
                self.bytes -= payload_bytes(&mutation);
            }
        }
    }

    pub(crate) fn take_waiter(&mut self) -> Option<std::task::Waker> {
        self.waiter.take().map(|waiter| waiter.waker)
    }

    pub(crate) fn register(&mut self) -> Result<u64, InboxError> {
        self.registration = self
            .registration
            .checked_add(1)
            .ok_or(InboxError::Registration)?;
        Ok(self.registration)
    }
}

// A wait exists only while the host drives an async operation. Clearing it on
// cancellation/completion prevents an idle inbox from retaining its host task.
struct RegisteredWaiter {
    owner: std::sync::Weak<()>,
    waker: std::task::Waker,
}
pub(crate) struct Waiter {
    inbox: Arc<std::sync::Mutex<Inbox>>,
    owner: Arc<()>,
}
impl Waiter {
    pub(crate) fn new(inbox: Arc<std::sync::Mutex<Inbox>>) -> Self {
        Self {
            inbox,
            owner: Arc::new(()),
        }
    }

    pub(crate) fn register(&self, cx: &std::task::Context<'_>) -> bool {
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.waiter = Some(RegisteredWaiter {
                owner: Arc::downgrade(&self.owner),
                waker: cx.waker().clone(),
            });
            return inbox.overflow || !inbox.areas.is_empty();
        }
        false
    }
}
impl Drop for Waiter {
    fn drop(&mut self) {
        if let Ok(mut inbox) = self.inbox.lock() {
            if inbox.waiter.as_ref().is_some_and(|waiter| {
                std::sync::Weak::ptr_eq(&waiter.owner, &Arc::downgrade(&self.owner))
            }) {
                inbox.waiter = None;
            }
        }
    }
}

/// Native-only readiness for an idle host worker; owns no VM or realm handles.
/// A session has one host driver; the most recently polled signal owns its wake.
pub struct StorageEventSignal {
    waiter: Option<Waiter>,
    ready: bool,
}
impl StorageEventSignal {
    pub(crate) fn new(waiter: Option<Waiter>, ready: bool) -> Self {
        Self { waiter, ready }
    }
}
impl std::future::Future for StorageEventSignal {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.ready
            || self
                .waiter
                .as_ref()
                .is_some_and(|waiter| waiter.register(cx))
        {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    }
}

pub(crate) fn payload_bytes(mutation: &Mutation) -> usize {
    mutation
        .key
        .as_ref()
        .map_or(0, Vec::len)
        .saturating_add(mutation.old.as_ref().map_or(0, Vec::len))
        .saturating_add(mutation.new.as_ref().map_or(0, Vec::len))
        .saturating_mul(2)
        .saturating_add(mutation.url.len())
}

pub(crate) fn broadcast_commit(
    subscribers: &[std::sync::Weak<std::sync::Mutex<Inbox>>],
    own: Option<&Arc<std::sync::Mutex<Inbox>>>,
    area: u32,
    mutation: &Arc<Mutation>,
    apply: impl FnOnce(),
) -> Result<Vec<std::task::Waker>, InboxError> {
    let bytes = payload_bytes(mutation);
    let mut inboxes = subscribers
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .filter(|inbox| own.is_none_or(|own| !Arc::ptr_eq(own, inbox)))
        .collect::<Vec<_>>();
    inboxes.sort_unstable_by_key(|inbox| Arc::as_ptr(inbox) as usize);
    inboxes.dedup_by(|left, right| Arc::ptr_eq(left, right));
    let mut guards = inboxes
        .iter()
        .map(|inbox| inbox.lock().map_err(|_| InboxError::Unavailable))
        .collect::<Result<Vec<_>, _>>()?;
    let interested = guards
        .iter_mut()
        .map(|inbox| inbox.interested(area))
        .collect::<Vec<_>>();
    for (inbox, count) in guards.iter().zip(&interested) {
        if *count != 0 {
            inbox.ensure_mutation_capacity(bytes, *count)?;
        }
    }
    apply();
    let mut wake = Vec::new();
    for (inbox, count) in guards.iter_mut().zip(interested) {
        if count != 0 {
            inbox.push_mutation(area, mutation.clone());
            if let Some(waiter) = inbox.take_waiter() {
                wake.push(waiter);
            }
        }
    }
    drop(guards);
    Ok(wake)
}

#[cfg(test)]
mod tests {
    use super::{broadcast_commit, Inbox, Membership, Mutation, Notice};
    use std::sync::{Arc, Mutex};

    fn member(inbox: &Arc<Mutex<Inbox>>, local: u32) -> Arc<Membership> {
        let membership = Arc::new(Membership {
            local,
            session: local + 100,
            active: std::sync::atomic::AtomicBool::new(true),
        });
        let mut inbox = inbox.lock().unwrap();
        inbox.register().unwrap();
        inbox.memberships.push(Arc::downgrade(&membership));
        membership
    }

    fn mutation() -> Arc<Mutation> {
        Arc::new(Mutation {
            key: Some(vec![0xd800]),
            old: None,
            new: Some(vec![0xdfff]),
            url: "source".into(),
        })
    }

    #[test]
    fn every_recipient_is_preflighted_before_commit_and_recovers() {
        let first = Arc::new(Mutex::new(Inbox::default()));
        let blocked = Arc::new(Mutex::new(Inbox::default()));
        let _members = [member(&first, 7), member(&blocked, 7)];
        for area in 0..8192 {
            blocked.lock().unwrap().push(area);
        }
        let subscribers = [Arc::downgrade(&first), Arc::downgrade(&blocked)];
        let applied = std::cell::Cell::new(false);
        let payload = mutation();
        assert!(broadcast_commit(&subscribers, None, 7, &payload, || applied.set(true)).is_err());
        assert!(!applied.get());
        assert_eq!(first.lock().unwrap().areas.len(), 0);
        assert_eq!(blocked.lock().unwrap().areas.len(), 8192);
        blocked.lock().unwrap().pop();
        broadcast_commit(&subscribers, None, 7, &payload, || applied.set(true)).unwrap();
        assert!(applied.get());
        let notice = first.lock().unwrap().next().unwrap().unwrap();
        assert!(Arc::ptr_eq(notice.mutation.as_ref().unwrap(), &payload));
        assert_eq!(notice.registration, 1);
        assert_eq!(notice.mutation.unwrap().key.as_ref().unwrap(), &[0xd800]);
        first.lock().unwrap().pop();
        assert_eq!(first.lock().unwrap().bytes, 0);
    }

    #[test]
    fn irrelevant_retired_and_own_inboxes_do_not_block_or_receive() {
        let own = Arc::new(Mutex::new(Inbox::default()));
        let unrelated = Arc::new(Mutex::new(Inbox::default()));
        let retired = Arc::new(Mutex::new(Inbox::default()));
        let _own_member = member(&own, 7);
        let _unrelated_member = member(&unrelated, 8);
        let retired_member = member(&retired, 7);
        retired_member
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        for area in 0..8192 {
            for inbox in [&own, &unrelated, &retired] {
                inbox.lock().unwrap().push(area);
            }
        }
        let subscribers = [
            Arc::downgrade(&own),
            Arc::downgrade(&unrelated),
            Arc::downgrade(&retired),
        ];
        let applied = std::cell::Cell::new(false);
        broadcast_commit(&subscribers, Some(&own), 7, &mutation(), || {
            applied.set(true)
        })
        .unwrap();
        assert!(applied.get());
        for inbox in [&own, &unrelated, &retired] {
            assert_eq!(inbox.lock().unwrap().areas.len(), 8192);
        }
        drop(retired_member);
        assert_eq!(retired.lock().unwrap().interested(7), 0);
        assert!(retired.lock().unwrap().memberships.is_empty());
    }

    #[test]
    fn payload_byte_budget_is_atomic_and_accepted_mutations_survive_clear_overflow() {
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let _member = member(&inbox, 7);
        let subscribers = [Arc::downgrade(&inbox)];
        let applied = std::cell::Cell::new(false);
        inbox.lock().unwrap().bytes = 64 * 1024 * 1024 - 1;
        assert!(
            broadcast_commit(&subscribers, None, 7, &mutation(), || applied.set(true)).is_err()
        );
        assert!(!applied.get());
        inbox.lock().unwrap().bytes = 0;
        let payload = mutation();
        broadcast_commit(&subscribers, None, 7, &payload, || applied.set(true)).unwrap();
        for _ in 0..8192 {
            inbox.lock().unwrap().push(7);
        }
        assert!(inbox.lock().unwrap().next().is_err());
        assert!(Arc::ptr_eq(
            inbox
                .lock()
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .mutation
                .as_ref()
                .unwrap(),
            &payload
        ));
        inbox.lock().unwrap().pop();
        assert_eq!(inbox.lock().unwrap().bytes, 0);
    }

    #[test]
    fn concurrent_native_publishers_use_one_lock_order() {
        let first = Arc::new(Mutex::new(Inbox::default()));
        let second = Arc::new(Mutex::new(Inbox::default()));
        let _members = [member(&first, 7), member(&second, 7)];
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut threads = Vec::new();
        for inboxes in [
            [first.clone(), second.clone()],
            [second.clone(), first.clone()],
        ] {
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let subscribers = inboxes.iter().map(Arc::downgrade).collect::<Vec<_>>();
                barrier.wait();
                for _ in 0..200 {
                    broadcast_commit(&subscribers, None, 7, &mutation(), || {}).unwrap();
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(first.lock().unwrap().areas.len(), 400);
        assert_eq!(second.lock().unwrap().areas.len(), 400);
    }

    #[test]
    fn overflow_preserves_accepted_notices_and_reports_once() {
        let mut inbox = Inbox::default();
        for area in 0..8192 {
            inbox.push(area);
        }
        inbox.push(8192);
        assert!(inbox.next().is_err());
        assert_eq!(inbox.areas.len(), 8192);
        for area in 0..8192 {
            let notice = Notice {
                area,
                registration: 0,
                mutation: None,
            };
            assert_eq!(inbox.next().unwrap(), Some(notice.clone()));
            assert_eq!(inbox.areas.pop_front(), Some(notice));
        }
        assert_eq!(inbox.next().unwrap(), None);
        inbox.push(9000);
        assert_eq!(
            inbox.next().unwrap(),
            Some(Notice {
                area: 9000,
                registration: 0,
                mutation: None
            })
        );
    }

    #[test]
    fn notices_capture_registration_before_later_realms() {
        let mut inbox = Inbox::default();
        assert_eq!(inbox.register().unwrap(), 1);
        inbox.push(7);
        assert_eq!(inbox.register().unwrap(), 2);
        assert_eq!(
            inbox.next().unwrap(),
            Some(Notice {
                area: 7,
                registration: 1,
                mutation: None
            })
        );
        inbox.registration = u64::MAX;
        assert!(inbox.register().is_err());
        assert_eq!(inbox.registration, u64::MAX);
    }
    #[test]
    fn waiters_wake_after_inbox_unlock_and_do_not_survive_operation_drop() {
        struct Wake {
            inbox: std::sync::Weak<Mutex<Inbox>>,
            count: std::sync::atomic::AtomicUsize,
            backing: std::sync::Weak<Mutex<()>>,
        }
        impl std::task::Wake for Wake {
            fn wake(self: Arc<Self>) {
                assert!(self.inbox.upgrade().unwrap().try_lock().is_ok());
                assert!(self.backing.upgrade().unwrap().try_lock().is_ok());
                self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let _member = member(&inbox, 7);
        let backing = Arc::new(Mutex::new(()));
        let wake = Arc::new(Wake {
            backing: Arc::downgrade(&backing),
            inbox: Arc::downgrade(&inbox),
            count: std::sync::atomic::AtomicUsize::new(0),
        });
        let waker = std::task::Waker::from(wake.clone());
        let cx = std::task::Context::from_waker(&waker);
        let waiting = super::Waiter::new(inbox.clone());
        waiting.register(&cx);
        let mutation = Arc::new(Mutation {
            key: None,
            old: None,
            new: None,
            url: String::new(),
        });
        let guard = backing.lock().unwrap();
        let wakes = broadcast_commit(&[Arc::downgrade(&inbox)], None, 7, &mutation, || {}).unwrap();
        assert_eq!(wake.count.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(guard);
        for waiter in wakes {
            waiter.wake();
        }
        assert_eq!(wake.count.load(std::sync::atomic::Ordering::SeqCst), 1);
        waiting.register(&cx);
        drop(waiting);
        assert!(inbox.lock().unwrap().waiter.is_none());
        broadcast_commit(&[Arc::downgrade(&inbox)], None, 7, &mutation, || {}).unwrap();
        assert_eq!(wake.count.load(std::sync::atomic::Ordering::SeqCst), 1);
        let weak = Arc::downgrade(&wake);
        drop(waker);
        drop(wake);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn older_wait_drop_does_not_clear_newer_signal_registration() {
        use std::future::Future;
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let first = super::Waiter::new(inbox.clone());
        let second = super::Waiter::new(inbox.clone());
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        first.register(&cx);
        let mut signal = super::StorageEventSignal::new(Some(second), false);
        assert!(std::pin::Pin::new(&mut signal).poll(&mut cx).is_pending());
        drop(first);
        assert!(inbox.lock().unwrap().waiter.is_some());
        inbox.lock().unwrap().push(7);
        assert!(std::pin::Pin::new(&mut signal).poll(&mut cx).is_ready());
        drop(signal);
        assert!(inbox.lock().unwrap().waiter.is_none());
    }
}
