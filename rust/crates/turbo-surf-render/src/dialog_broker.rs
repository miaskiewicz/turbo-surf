//! Send-only dialog requests; no V8 value crosses the host boundary.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

type ReplyResult = Result<DialogReply, String>;
type ReplyReceiver = mpsc::Receiver<ReplyResult>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogRequest {
    pub id: u64,
    pub owner: String,
    pub primary: bool,
    pub kind: DialogKind,
    pub message: Vec<u16>,
    pub default_value: Vec<u16>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogReply {
    pub accepted: bool,
    pub prompt_text: Option<Vec<u16>>,
}
impl DialogReply {
    pub fn dismissed() -> Self {
        Self {
            accepted: false,
            prompt_text: None,
        }
    }
}
#[derive(Clone, Default)]
pub struct DialogBroker(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    next: u64,
    closed: bool,
    primary_listener: bool,
    primary_closed: bool,
    context_listener: bool,
    listeners: BTreeMap<String, bool>,
    retired: BTreeSet<String>,
    pending: BTreeMap<u64, Pending>,
}
struct Pending {
    owner: String,
    primary: bool,
    reply: mpsc::SyncSender<ReplyResult>,
}
struct PendingGuard {
    broker: DialogBroker,
    id: u64,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.broker
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .remove(&self.id);
    }
}
impl DialogBroker {
    fn state(&self) -> Result<MutexGuard<'_, State>, String> {
        self.0.lock().map_err(|_| "Dialog broker poisoned".into())
    }
    pub fn configure(&self, owner: &str, enabled: bool) -> Result<(), String> {
        let mut state = self.state()?;
        if state.closed || state.retired.contains(owner) {
            return Err("Dialog target closed".into());
        }
        state.listeners.insert(owner.into(), enabled);
        Ok(())
    }
    pub fn configure_primary(&self, enabled: bool) -> Result<(), String> {
        let mut state = self.state()?;
        if state.closed || state.primary_closed {
            return Err("Dialog actor closed".into());
        }
        state.primary_listener = enabled;
        Ok(())
    }
    pub fn configure_context(&self, enabled: bool) -> Result<(), String> {
        let mut state = self.state()?;
        if state.closed {
            return Err("Dialog actor closed".into());
        }
        state.context_listener = enabled;
        Ok(())
    }
    pub fn request(
        &self,
        mut request: DialogRequest,
        deadline: impl Into<Option<Instant>>,
        mut cancelled: impl FnMut() -> bool,
        publish: impl FnOnce(DialogRequest),
    ) -> Result<DialogReply, String> {
        let deadline = deadline.into();
        let Some(receiver) = self.begin_request(&mut request)? else {
            return Ok(DialogReply::dismissed());
        };
        let _pending = PendingGuard {
            broker: self.clone(),
            id: request.id,
        };
        publish(request);
        loop {
            if cancelled() {
                return Err("Dialog execution cancelled".into());
            }
            let remaining = match deadline {
                Some(deadline) => deadline
                    .checked_duration_since(Instant::now())
                    .ok_or("Dialog timeout")?,
                None => Duration::from_millis(5),
            };
            match receiver.recv_timeout(remaining.min(Duration::from_millis(5))) {
                Ok(reply) => return reply,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Dialog host disconnected".into())
                }
            }
        }
    }
    fn begin_request(&self, request: &mut DialogRequest) -> Result<Option<ReplyReceiver>, String> {
        let mut state = self.state()?;
        if state.closed
            || (request.primary && state.primary_closed)
            || state.retired.contains(&request.owner)
        {
            return Err("Dialog target closed".into());
        }
        let enabled = state.context_listener
            || (request.primary && state.primary_listener)
            || state
                .listeners
                .get(&request.owner)
                .copied()
                .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        if state.pending.len() >= 64 {
            return Err("Dialog request capacity exceeded".into());
        }
        request.id = state
            .next
            .checked_add(1)
            .ok_or("Dialog request IDs exhausted")?;
        state.next = request.id;
        let (reply, receiver) = mpsc::sync_channel(1);
        state.pending.insert(
            request.id,
            Pending {
                owner: request.owner.clone(),
                primary: request.primary,
                reply,
            },
        );
        Ok(Some(receiver))
    }
    pub fn respond(&self, owner: &str, id: u64, reply: DialogReply) -> Result<(), String> {
        let pending = {
            let mut state = self.state()?;
            if state
                .pending
                .get(&id)
                .is_none_or(|pending| pending.owner != owner)
            {
                return Err("No pending dialog for target".into());
            }
            state.pending.remove(&id).unwrap()
        };
        pending
            .reply
            .try_send(Ok(reply))
            .map_err(|_| "Dialog already completed".into())
    }
    pub fn respond_primary(&self, id: u64, reply: DialogReply) -> Result<(), String> {
        let owner = self
            .state()?
            .pending
            .get(&id)
            .filter(|pending| pending.primary)
            .map(|pending| pending.owner.clone())
            .ok_or("No pending primary dialog")?;
        self.respond(&owner, id, reply)
    }
    pub fn cancel_owner(&self, owner: &str) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.retired.insert(owner.into());
        state.listeners.remove(owner);
        state.pending.retain(|_, pending| {
            if pending.owner != owner {
                return true;
            }
            let _ = pending.reply.try_send(Err("Dialog target closed".into()));
            false
        });
    }
    pub fn cancel_primary(&self) {
        let owners = {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.primary_closed = true;
            state
                .pending
                .values()
                .filter(|pending| pending.primary)
                .map(|pending| pending.owner.clone())
                .collect::<Vec<_>>()
        };
        for owner in owners {
            self.cancel_owner(&owner);
        }
    }
    pub fn close(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        state.listeners.clear();
        for (_, pending) in std::mem::take(&mut state.pending) {
            let _ = pending.reply.try_send(Err("Dialog actor closed".into()));
        }
    }
}

#[cfg(test)]
#[path = "dialog_broker_tests.rs"]
mod tests;
