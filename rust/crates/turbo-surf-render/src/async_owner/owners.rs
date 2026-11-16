//! Native weak document leases, private to the stable infrastructure realm.
use super::work::OwnerId;
use deno_core::v8;
use std::collections::HashMap;

pub struct WeakOwners {
    key: v8::Global<v8::Private>,
    infrastructure: v8::Weak<v8::Context>,
    sequence: u32,
    contexts: HashMap<OwnerId, Lease>,
}

struct Lease {
    context: Option<v8::Weak<v8::Context>>,
    token: v8::Weak<v8::Object>,
}

pub enum LeaseState {
    Live(OwnerId),
    Retired(OwnerId),
    Unknown,
}

impl WeakOwners {
    pub fn new(scope: &mut v8::PinScope) -> Self {
        let key = v8::Private::new(scope, None);
        let infrastructure = scope.get_current_context();
        Self {
            key: v8::Global::new(scope, key),
            infrastructure: v8::Weak::new(scope, infrastructure),
            sequence: 0,
            contexts: HashMap::new(),
        }
    }

    pub fn register<'s>(
        &mut self,
        scope: &mut v8::PinScope<'s, '_>,
        authenticated_context: v8::Local<'s, v8::Context>,
        valid_native_document: impl FnOnce(
            &mut v8::PinScope<'s, '_>,
            v8::Local<'s, v8::Context>,
        ) -> bool,
    ) -> Option<(OwnerId, v8::Local<'s, v8::Object>)> {
        if !valid_native_document(scope, authenticated_context) {
            return None;
        }
        if let Some(existing) = self.existing(scope, authenticated_context) {
            return Some(existing);
        }
        self.sequence = self.sequence.checked_add(1)?;
        let owner = OwnerId::registered(u64::from(self.sequence));
        let guest = v8::Weak::new(scope, authenticated_context);
        let token = self.create_token(scope)?;
        let token = v8::Local::new(scope, token);
        let key = v8::Local::new(scope, &self.key);
        {
            let scope = &mut v8::ContextScope::new(scope, authenticated_context);
            // The document owns its E token, not vice versa. This keeps the
            // native identity stable while the document is live, without an E
            // async snapshot retaining a guest prototype/context through it.
            if authenticated_context
                .global(scope)
                .set_private(scope, key, token.into())
                != Some(true)
            {
                return None;
            }
        }
        self.contexts.insert(
            owner,
            Lease {
                context: Some(guest),
                token: v8::Weak::new(scope, token),
            },
        );
        Some((owner, token))
    }

    fn create_token<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Global<v8::Object>> {
        let infrastructure = self.infrastructure.to_local(scope)?;
        let scope = &mut v8::ContextScope::new(scope, infrastructure);
        // A token is retained in async snapshots. Its prototype must belong to
        // E: a guest-created Object would itself strongly retain that guest.
        let token = v8::Object::new(scope);
        let key = v8::Local::new(scope, &self.key);
        let value = v8::Integer::new_from_unsigned(scope, self.sequence);
        if token.set_private(scope, key, value.into()) != Some(true) {
            return None;
        }
        Some(v8::Global::new(scope, token))
    }

    fn existing<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        context: v8::Local<'s, v8::Context>,
    ) -> Option<(OwnerId, v8::Local<'s, v8::Object>)> {
        let key = v8::Local::new(scope, &self.key);
        let scope = &mut v8::ContextScope::new(scope, context);
        let token = context.global(scope).get_private(scope, key)?;
        let token = v8::Local::<v8::Object>::try_from(token).ok()?;
        let id = token
            .get_private(scope, key)
            .filter(|value| value.is_uint32())
            .and_then(|value| value.uint32_value(scope))?;
        let owner = OwnerId::registered(u64::from(id));
        let lease = self.contexts.get(&owner)?;
        (lease.token.to_local(scope) == Some(token)
            && lease.context.as_ref()?.to_local(scope) == Some(context))
        .then_some((owner, token))
    }

    pub fn resolve<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        token: v8::Local<'s, v8::Value>,
        valid_native_document: impl FnOnce(
            &mut v8::PinScope<'s, '_>,
            v8::Local<'s, v8::Context>,
        ) -> bool,
    ) -> LeaseState {
        let Ok(token) = v8::Local::<v8::Object>::try_from(token) else {
            return LeaseState::Unknown;
        };
        if token.is_proxy() {
            return LeaseState::Unknown;
        }
        let key = v8::Local::new(scope, &self.key);
        let Some(id) = token
            .get_private(scope, key)
            .filter(|value| value.is_uint32())
            .and_then(|value| value.uint32_value(scope))
        else {
            return LeaseState::Unknown;
        };
        let owner = OwnerId::registered(u64::from(id));
        let Some(lease) = self.contexts.get(&owner) else {
            return LeaseState::Unknown;
        };
        if lease.token.to_local(scope) != Some(token) {
            return LeaseState::Unknown;
        }
        let context = lease
            .context
            .as_ref()
            .and_then(|context| context.to_local(scope));
        match context {
            Some(context) if valid_native_document(scope, context) => LeaseState::Live(owner),
            _ => LeaseState::Retired(owner),
        }
    }

    pub fn identify<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        context: v8::Local<'s, v8::Context>,
        valid_native_document: impl FnOnce(
            &mut v8::PinScope<'s, '_>,
            v8::Local<'s, v8::Context>,
        ) -> bool,
    ) -> LeaseState {
        let owner = self.contexts.iter().find_map(|(owner, lease)| {
            (lease.context.as_ref()?.to_local(scope) == Some(context)).then_some(*owner)
        });
        match owner {
            Some(owner) if valid_native_document(scope, context) => LeaseState::Live(owner),
            Some(owner) => LeaseState::Retired(owner),
            None => LeaseState::Unknown,
        }
    }

    /// Convert selected opaque owners back to actual live native contexts for
    /// the embedder's document-task selector. No Context is retained strongly.
    pub fn context<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        owner: OwnerId,
        valid_native_document: impl FnOnce(
            &mut v8::PinScope<'s, '_>,
            v8::Local<'s, v8::Context>,
        ) -> bool,
    ) -> Option<v8::Local<'s, v8::Context>> {
        let context = self
            .contexts
            .get(&owner)?
            .context
            .as_ref()?
            .to_local(scope)?;
        valid_native_document(scope, context).then_some(context)
    }

    pub fn prune(&mut self, scope: &mut v8::PinScope) {
        self.contexts
            .retain(|_, lease| lease.token.to_local(scope).is_some());
    }
}
