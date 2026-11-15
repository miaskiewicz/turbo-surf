//! Actor-local acknowledgement of native history changes before guest observers run.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Identity {
    pub target: u64,
    pub document: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Ticket {
    identity: Identity,
    token: u64,
}

impl Ticket {
    pub fn token(self) -> String {
        self.token.to_string()
    }
}

struct Pending {
    ticket: Ticket,
    committed: bool,
}

#[derive(Default)]
pub(super) struct Transactions {
    serial: u64,
    pending: BTreeMap<u64, Pending>,
}

impl Transactions {
    /// Call before mutation, after all guest getters and fallible preparation.
    pub fn begin(&mut self, identity: Identity) -> Result<Ticket, String> {
        if identity.document == 0 {
            return Err("History transaction document identity is unavailable".into());
        }
        if self.pending.contains_key(&identity.target) {
            return Err("History target has an unacknowledged transaction".into());
        }
        let token = self
            .serial
            .checked_add(1)
            .ok_or("History transaction token exhausted")?;
        let ticket = Ticket { identity, token };
        self.pending.insert(
            identity.target,
            Pending {
                ticket,
                committed: false,
            },
        );
        self.serial = token;
        Ok(ticket)
    }

    /// No guest code or fallible work may occur between mutation and this marker.
    pub fn committed(&mut self, ticket: Ticket) -> Result<(), String> {
        let pending = self.exact_mut(ticket)?;
        pending.committed = true;
        Ok(())
    }

    /// Only a failed native precommit may abort; mutated state requires guest ACK.
    pub fn abort(&mut self, ticket: Ticket) -> Result<(), String> {
        if self.exact_mut(ticket)?.committed {
            return Err("Committed history transaction requires acknowledgement".into());
        }
        self.pending.remove(&ticket.identity.target);
        Ok(())
    }

    /// The caller authenticates the captured actual Document before entering here.
    pub fn ack(&mut self, identity: Identity, token: &str) -> Result<(), String> {
        let pending = self
            .pending
            .get(&identity.target)
            .ok_or("History transaction is stale")?;
        if pending.ticket.identity != identity || pending.ticket.token.to_string() != token {
            return Err("History transaction ownership mismatch".into());
        }
        if !pending.committed {
            return Err("History transaction has not committed".into());
        }
        self.pending.remove(&identity.target);
        Ok(())
    }

    /// Called at a failed command boundary, never as a general timeout-close policy.
    pub fn take_unacknowledged(&mut self) -> Vec<Identity> {
        let pending = std::mem::take(&mut self.pending);
        pending
            .into_values()
            .filter(|entry| entry.committed)
            .map(|entry| entry.ticket.identity)
            .collect()
    }

    fn exact_mut(&mut self, ticket: Ticket) -> Result<&mut Pending, String> {
        let pending = self
            .pending
            .get_mut(&ticket.identity.target)
            .ok_or("History transaction is stale")?;
        if pending.ticket != ticket {
            return Err("History transaction ownership mismatch".into());
        }
        Ok(pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(target: u64, document: u64) -> Identity {
        Identity { target, document }
    }

    #[test]
    fn ack_requires_exact_document_target_and_token_and_is_one_shot() {
        let mut transactions = Transactions::default();
        let identity = owner(0, 20);
        let ticket = transactions.begin(identity).unwrap();
        assert!(transactions.ack(identity, &ticket.token()).is_err());
        transactions.committed(ticket).unwrap();
        assert!(transactions.ack(owner(1, 20), &ticket.token()).is_err());
        assert!(transactions.ack(owner(0, 21), &ticket.token()).is_err());
        assert!(transactions.ack(identity, "01").is_err());
        transactions.ack(identity, &ticket.token()).unwrap();
        assert!(transactions.ack(identity, &ticket.token()).is_err());
        assert!(transactions.take_unacknowledged().is_empty());
    }

    #[test]
    fn interrupted_commit_retires_only_affected_unacked_targets() {
        let mut transactions = Transactions::default();
        let affected = transactions.begin(owner(1, 11)).unwrap();
        let peer = transactions.begin(owner(2, 22)).unwrap();
        transactions.committed(affected).unwrap();
        transactions.committed(peer).unwrap();
        transactions.ack(owner(2, 22), &peer.token()).unwrap();
        transactions.begin(owner(3, 33)).unwrap();
        assert_eq!(transactions.take_unacknowledged(), vec![owner(1, 11)]);
        assert!(transactions.take_unacknowledged().is_empty());
    }

    #[test]
    fn failed_precommit_preserves_recovery_and_stale_tickets_cannot_mutate_new_work() {
        let mut transactions = Transactions::default();
        let identity = owner(5, 55);
        let first = transactions.begin(identity).unwrap();
        assert!(transactions.begin(identity).is_err());
        transactions.abort(first).unwrap();
        let second = transactions.begin(identity).unwrap();
        assert_ne!(first.token(), second.token());
        assert!(transactions.committed(first).is_err());
        transactions.committed(second).unwrap();
        assert!(transactions.abort(second).is_err());
        assert!(transactions.ack(identity, &first.token()).is_err());
        transactions.ack(identity, &second.token()).unwrap();
    }
}
