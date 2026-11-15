//! One-shot native handoff when a static document acquires its live realm.
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};
use turbo_surf_core::net::FetchedBytes;
use turbo_surf_render::InitialDocumentImages;

#[derive(Clone, Default)]
pub(crate) struct DocumentImages(Arc<Mutex<Option<Document>>>);
struct Document {
    token: u32,
    source: String,
    assets: InitialDocumentImages,
}
impl DocumentImages {
    pub(crate) fn begin(&self, source: String) -> Option<u32> {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let token = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok()?;
        *self.0.lock().ok()? = Some(Document {
            token,
            source,
            assets: InitialDocumentImages::default(),
        });
        Some(token)
    }
    pub(crate) fn record(
        &self,
        token: Option<u32>,
        source: &str,
        requested: &str,
        response: FetchedBytes,
    ) {
        let Ok(mut current) = self.0.lock() else {
            return;
        };
        let Some(doc) = current
            .as_mut()
            .filter(|doc| Some(doc.token) == token && doc.source == source)
        else {
            return;
        };
        doc.assets.insert(source, requested, response);
    }
    pub(crate) fn take(&self, token: Option<u32>, source: &str) -> InitialDocumentImages {
        let Ok(mut current) = self.0.lock() else {
            return InitialDocumentImages::default();
        };
        if current
            .as_ref()
            .is_some_and(|doc| Some(doc.token) == token && doc.source == source)
        {
            return current.take().unwrap().assets;
        }
        InitialDocumentImages::default()
    }
    pub(crate) fn discard(&self, token: u32) {
        if let Ok(mut current) = self.0.lock() {
            if current.as_ref().is_some_and(|doc| doc.token == token) {
                *current = None;
            }
        }
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut current) = self.0.lock() {
            *current = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_capabilities_are_owner_bound_retired_and_consumed_once() {
        let owner = DocumentImages::default();
        let foreign = DocumentImages::default();
        let first = owner.begin("https://site.test/".into()).unwrap();
        let other = foreign.begin("https://site.test/".into()).unwrap();
        assert_ne!(first, other);
        foreign.discard(first);
        assert!(foreign.0.lock().unwrap().is_some());
        foreign.take(Some(first), "https://site.test/");
        assert!(foreign.0.lock().unwrap().is_some());
        owner.take(Some(first), "https://wrong.test/");
        assert!(owner.0.lock().unwrap().is_some());
        let next = owner.begin("https://site.test/".into()).unwrap();
        owner.discard(first);
        owner.take(Some(first), "https://site.test/");
        assert_eq!(owner.0.lock().unwrap().as_ref().unwrap().token, next);
        owner.take(Some(next), "https://site.test/");
        assert!(owner.0.lock().unwrap().is_none());
        owner.take(Some(next), "https://site.test/");
        assert!(owner.0.lock().unwrap().is_none());
        owner.begin("https://site.test/".into());
        owner.clear();
        assert!(owner.0.lock().unwrap().is_none());
    }
    #[test]
    fn late_completed_capture_cannot_repopulate_retired_or_closed_document() {
        let owner = DocumentImages::default();
        let retired = owner.begin("https://site.test/".into()).unwrap();
        let current = owner.begin("https://site.test/".into()).unwrap();
        let response = FetchedBytes {
            final_url: "https://site.test/a.png".into(),
            bytes: vec![],
            status: 200,
            content_type: "image/png".into(),
        };
        owner.record(
            Some(retired),
            "https://site.test/",
            "https://site.test/a.png",
            response.clone(),
        );
        assert_eq!(owner.0.lock().unwrap().as_ref().unwrap().token, current);
        owner.clear();
        owner.record(
            Some(current),
            "https://site.test/",
            "https://site.test/a.png",
            response,
        );
        assert!(owner.0.lock().unwrap().is_none());
    }
}
