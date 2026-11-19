//! Current-document image bodies for trusted screenshot capture, never guest JS.
use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

const MAX_RECORDS: usize = 60;
const MAX_BYTES: usize = 64 * 1024 * 1024;
pub(super) type Assets = Rc<RefCell<DocumentAssets>>;

pub(super) struct DocumentAssets {
    active: bool,
    revision: Option<u64>,
    pending: BTreeMap<u32, Pending>,
    resident: VecDeque<Resident>,
}
impl Default for DocumentAssets {
    fn default() -> Self {
        Self {
            active: true,
            revision: Some(0),
            pending: BTreeMap::new(),
            resident: VecDeque::new(),
        }
    }
}
struct Pending {
    image: v8::Weak<v8::Object>,
    reference: String,
    requested_url: String,
    url: String,
    bytes: Option<Arc<[u8]>>,
    finished: bool,
    claimed: bool,
}
struct Resident {
    image: v8::Weak<v8::Object>,
    reference: String,
    requested_url: String,
    url: String,
    bytes: Arc<[u8]>,
}
fn allocate_token(counter: &std::sync::atomic::AtomicU32) -> Option<u32> {
    use std::sync::atomic::Ordering;
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .ok()
}
fn next_token() -> Option<u32> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    allocate_token(&NEXT)
}
fn admits_bytes(active: bool, retained: usize, incoming: usize) -> bool {
    active && retained <= MAX_BYTES && incoming <= MAX_BYTES - retained
}

fn context_assets(context: v8::Local<v8::Context>) -> Option<Assets> {
    context
        .get_slot::<CspHandle>()
        .map(|handle| handle.image_assets.clone())
}
fn image_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    image: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Context>> {
    use turbo_dom_browser_env::host_internals::{
        current_document_context, enter_document_of, handle_of, is_connected, with_tree,
    };
    let _document = enter_document_of(scope, image);
    let handle = handle_of(scope, image)?;
    let connected =
        with_tree(|tree| tree.local_name(handle) == Some("img") && is_connected(tree, handle))?;
    let context = current_document_context(scope)?;
    (connected && context.get_slot::<FrameRetired>().is_none()).then_some(context)
}

fn document_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    node: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Context>> {
    let _document = turbo_dom_browser_env::host_internals::enter_document_of(scope, node);
    turbo_dom_browser_env::host_internals::current_document_context(scope)
}

#[op2(fast)]
pub(super) fn op_begin_image_asset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    image: v8::Local<'s, v8::Object>,
    #[string] reference: String,
    #[string] requested_url: String,
) -> u32 {
    let Some(owner) = image_owner(scope, image).and_then(context_assets) else {
        return 0;
    };
    let mut owner = owner.borrow_mut();
    owner.pending.retain(|_, request| {
        request
            .image
            .to_local(scope)
            .is_some_and(|value| value != image)
    });
    owner.resident.retain(|entry| {
        entry
            .image
            .to_local(scope)
            .is_some_and(|value| value != image)
    });
    owner.revision = owner.revision.and_then(|value| value.checked_add(1));
    if !owner.active || owner.pending.len() + owner.resident.len() >= MAX_RECORDS {
        return 0;
    }
    let Some(token) = next_token() else {
        return 0;
    };
    owner.pending.insert(
        token,
        Pending {
            image: v8::Weak::new(scope, image),
            reference,
            requested_url,
            url: String::new(),
            bytes: None,
            finished: false,
            claimed: false,
        },
    );
    token
}

pub(super) struct Request {
    owner: Option<Assets>,
    token: u32,
}
impl Request {
    pub(super) fn new(state: &OpState, csp: u32, token: u32, url: &str) -> Self {
        let owner = state
            .borrow::<CspContexts>()
            .0
            .get(&csp)
            .map(|handle| handle.image_assets.clone());
        let owner = owner.filter(|owner| {
            let mut owner = owner.borrow_mut();
            if !owner.active {
                return false;
            }
            let Some(request) = owner.pending.get_mut(&token) else {
                return false;
            };
            if request.claimed || request.requested_url != url {
                return false;
            }
            request.claimed = true;
            true
        });
        Self { owner, token }
    }
    pub(super) fn finish(&mut self, url: String, bytes: Vec<u8>) {
        let Some(owner) = &self.owner else {
            return;
        };
        let mut owner = owner.borrow_mut();
        let retained: usize = owner
            .pending
            .values()
            .filter_map(|entry| entry.bytes.as_ref())
            .map(|bytes| bytes.len())
            .sum::<usize>()
            + owner
                .resident
                .iter()
                .map(|entry| entry.bytes.len())
                .sum::<usize>();
        let admitted = admits_bytes(owner.active, retained, bytes.len());
        if let Some(request) = owner.pending.get_mut(&self.token) {
            request.url = url;
            request.finished = true;
            if admitted {
                request.bytes = Some(Arc::from(bytes));
            }
            owner.revision = owner.revision.and_then(|value| value.checked_add(1));
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        if let Some(owner) = &self.owner {
            let mut owner = owner.borrow_mut();
            if let Some(request) = owner.pending.get_mut(&self.token) {
                if !request.finished {
                    request.finished = true;
                    owner.revision = owner.revision.and_then(|value| value.checked_add(1));
                }
            }
        }
    }
}

#[op2(fast)]
pub(super) fn op_commit_image_asset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    image: v8::Local<'s, v8::Object>,
    token: u32,
) {
    let Some(context) = document_owner(scope, image) else {
        return;
    };
    let Some(owner) = context_assets(context) else {
        return;
    };
    let mut owner = owner.borrow_mut();
    let valid = owner.active
        && owner.pending.get(&token).is_some_and(|request| {
            request.finished && request.image.to_local(scope) == Some(image)
        });
    if !valid {
        return;
    }
    let request = owner.pending.remove(&token).unwrap();
    if image_owner(scope, image) != Some(context) {
        return;
    }
    owner.resident.retain(|entry| {
        entry
            .image
            .to_local(scope)
            .is_some_and(|value| value != image)
    });
    if let Some(bytes) = request.bytes {
        while owner.resident.len() >= MAX_RECORDS {
            owner.resident.pop_front();
        }
        owner.resident.push_back(Resident {
            image: request.image,
            reference: request.reference,
            requested_url: request.requested_url,
            url: request.url,
            bytes,
        });
    }
    owner.revision = owner.revision.and_then(|value| value.checked_add(1));
}

#[op2(fast)]
pub(super) fn op_discard_image_asset(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    token: u32,
) {
    let _document = turbo_dom_browser_env::host_internals::enter_document_of(scope, document);
    if let Some(owner) = turbo_dom_browser_env::host_internals::current_document_context(scope)
        .and_then(context_assets)
    {
        owner.borrow_mut().pending.remove(&token);
    }
}

pub(crate) fn clear_document(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) {
    let _document = turbo_dom_browser_env::host_internals::enter_document_of(scope, document);
    let context = turbo_dom_browser_env::host_internals::current_document_context(scope);
    if let Some(context) = context {
        initial_document_images::retire(scope, context);
    }
    if let Some(owner) = context.and_then(context_assets) {
        let mut owner = owner.borrow_mut();
        owner.pending.clear();
        owner.resident.clear();
        owner.revision = owner.revision.and_then(|value| value.checked_add(1));
    }
}
pub(super) fn retire(context: v8::Local<v8::Context>) {
    if let Some(owner) = context_assets(context) {
        let mut owner = owner.borrow_mut();
        owner.active = false;
        owner.pending.clear();
        owner.resident.clear();
        owner.revision = owner.revision.and_then(|value| value.checked_add(1));
    }
}

pub(super) fn clear_runtime(state: &OpState) {
    if state.has::<CspContexts>() {
        for handle in state.borrow::<CspContexts>().0.values() {
            let mut owner = handle.image_assets.borrow_mut();
            owner.active = false;
            owner.pending.clear();
            owner.resident.clear();
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Query {
    refs: Vec<String>,
    generation: Option<String>,
    #[serde(default)]
    cached_refs: Vec<String>,
}
#[derive(serde::Serialize)]
struct Snapshot {
    generation: Option<String>,
    images: BTreeMap<String, String>,
}
fn query(source: &str) -> Result<Query, String> {
    let query: Query =
        serde_json::from_str(source).map_err(|_| "Invalid screenshot asset query")?;
    if query.refs.len() > MAX_RECORDS || query.cached_refs.len() > MAX_RECORDS {
        return Err("Screenshot asset query exceeds resource limit".into());
    }
    Ok(query)
}
fn snapshot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    query: Query,
) -> Result<String, String> {
    if context.get_slot::<FrameRetired>().is_some() {
        return Err("Image document has retired".into());
    }
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return serde_json::to_string(&Snapshot {
            generation: None,
            images: BTreeMap::new(),
        })
        .map_err(|error| error.to_string());
    };
    let owner = handle.image_assets.borrow();
    if !owner.active {
        return Err("Image document has retired".into());
    }
    let generation = owner
        .revision
        .map(|revision| format!("{}:{revision}", handle.id));
    let same = generation.is_some() && query.generation == generation;
    let wanted = query
        .refs
        .iter()
        .filter(|reference| !same || !query.cached_refs.contains(reference))
        .collect::<Vec<_>>();
    let images = collect_images(scope, context, &owner.resident, &wanted);
    serde_json::to_string(&Snapshot { generation, images }).map_err(|error| error.to_string())
}
fn collect_images<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    entries: &VecDeque<Resident>,
    refs: &[&String],
) -> BTreeMap<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mut out = BTreeMap::new();
    for entry in entries {
        let Some(image) = entry.image.to_local(scope) else {
            continue;
        };
        if image_owner(scope, image) != Some(context) {
            continue;
        }
        for reference in refs {
            if *reference == &entry.reference
                || *reference == &entry.requested_url
                || *reference == &entry.url
            {
                out.insert((*reference).clone(), STANDARD.encode(&entry.bytes));
            }
        }
    }
    out
}
impl PageSession {
    /// Trusted host image data; no guest script or guest serializer is invoked.
    pub fn screenshot_image_assets(&mut self, source: &str) -> Result<String, String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        if let Some(target) = self.replaced_root_target() {
            return self.screenshot_target_image_assets(target, source);
        }
        let query = query(source)?;
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = scope.get_current_context();
        snapshot(scope, context, query)
    }
    /// Resolve the actual retained target rather than an opener or retired main realm.
    pub fn screenshot_target_image_assets(
        &mut self,
        target: u64,
        source: &str,
    ) -> Result<String, String> {
        let query = query(source)?;
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = auxiliary_pages::context(
            scope,
            turbo_dom_browser_env::host_internals::AuxiliaryContextId(target),
        )?;
        snapshot(scope, context, query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_and_resident_bodies_share_one_finite_byte_budget() {
        assert!(admits_bytes(true, 0, MAX_BYTES));
        assert!(admits_bytes(true, MAX_BYTES - 1, 1));
        assert!(!admits_bytes(true, MAX_BYTES, 1));
        assert!(!admits_bytes(true, MAX_BYTES + 1, 0));
        assert!(!admits_bytes(false, 0, 1));
    }
    #[test]
    fn asset_queries_preserve_the_document_resource_bound() {
        let refs = vec!["/image.png"; MAX_RECORDS];
        let accepted = serde_json::json!({"refs": refs, "cachedRefs": refs});
        assert!(query(&accepted.to_string()).is_ok());
        let excess = vec!["/image.png"; MAX_RECORDS + 1];
        assert!(query(&serde_json::json!({"refs": excess}).to_string()).is_err());
        assert!(query(&serde_json::json!({"refs": [], "cachedRefs": excess}).to_string()).is_err());
        assert!(query("{invalid").is_err());
    }
    #[test]
    fn exhausted_asset_tokens_never_wrap_or_reauthorize_an_old_request() {
        let counter = std::sync::atomic::AtomicU32::new(u32::MAX - 1);
        assert_eq!(allocate_token(&counter), Some(u32::MAX - 1));
        assert_eq!(allocate_token(&counter), None);
        assert_eq!(allocate_token(&counter), None);
    }
}
