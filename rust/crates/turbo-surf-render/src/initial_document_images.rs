//! Completed static-document image responses, transferred once to its native realm.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct InitialDocumentImages {
    pub(crate) source: String,
    bytes: usize,
    images: HashMap<String, FetchedBytes>,
}
impl InitialDocumentImages {
    /// Host-only capture. Redirected responses must be excluded by the caller.
    /// Conservative admission deliberately excludes SVG and foreign/opaque origins.
    pub fn insert(&mut self, source: &str, requested: &str, response: FetchedBytes) {
        if !admissible(source, requested, &response)
            || (!self.source.is_empty() && self.source != source)
            || self.images.len() >= 60
            || self.bytes.saturating_add(response.bytes.len()) > 64 * 1024 * 1024
        {
            return;
        }
        self.source = source.to_owned();
        if !self.images.contains_key(requested) {
            self.bytes += response.bytes.len();
            self.images.insert(requested.to_owned(), response);
        }
    }
}
fn admissible(source: &str, requested: &str, response: &FetchedBytes) -> bool {
    let origin = turbo_surf_core::request_context::origin(source);
    let svg = response
        .content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("image/svg+xml"));
    response.final_url == requested
        && (200..300).contains(&response.status)
        && !svg
        && origin.is_some()
        && origin == turbo_surf_core::request_context::origin(requested)
        && natural_image_size(&response.bytes, &response.content_type).is_some()
}
#[derive(Default)]
pub(super) struct PromotionImages {
    pending: InitialDocumentImages,
    owner: Option<u32>,
}
impl PromotionImages {
    pub(super) fn new(pending: InitialDocumentImages) -> Self {
        Self {
            pending,
            owner: None,
        }
    }
    pub(super) fn bind(&mut self, csp: u32, base: &str) {
        if self.owner.is_none() && self.pending.source == base {
            self.owner = Some(csp);
        }
    }
    fn retire(&mut self, csp: u32) {
        if self.owner == Some(csp) {
            self.pending = InitialDocumentImages::default();
        }
    }
    pub(super) fn get(
        &self,
        csp: u32,
        source: &str,
        target: &str,
        context: &RequestContext,
        max_bytes: Option<usize>,
    ) -> Option<FetchedBytes> {
        if self.owner != Some(csp)
            || self.pending.source != source
            || context.opaque_origin
            || context.credentialless
        {
            return None;
        }
        self.pending
            .images
            .get(target)
            .filter(|image| max_bytes.is_none_or(|limit| image.bytes.len() <= limit))
            .cloned()
    }
}

pub(super) fn retire(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    let Some(csp) = context.get_slot::<CspHandle>() else {
        return;
    };
    let state = JsRuntime::op_state_from(scope);
    if let Some(images) = state.borrow_mut().try_borrow_mut::<PromotionImages>() {
        images.retire(csp.id);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(url: &str) -> FetchedBytes {
        FetchedBytes {
            final_url: url.into(),
            content_type: "image/png".into(),
            status: 200,
            bytes: vec![
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 4, 0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252,
                255, 31, 0, 3, 3, 2, 0, 239, 163, 7, 105, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
                130,
            ],
        }
    }
    #[test]
    fn assets_require_actual_direct_same_origin_rasters_and_bounded_document() {
        let mut images = InitialDocumentImages::default();
        let source = "https://site.test/page";
        for index in 0..61 {
            let url = format!("https://site.test/{index}.png");
            images.insert(source, &url, image(&url));
        }
        assert_eq!(images.images.len(), 60);
        let mut rejected = InitialDocumentImages::default();
        rejected.insert(
            source,
            "https://other.test/a.png",
            image("https://other.test/a.png"),
        );
        rejected.insert(
            source,
            "https://site.test/a.png",
            image("https://site.test/redirected.png"),
        );
        let mut svg = image("https://site.test/a.png");
        svg.content_type = "image/svg+xml".into();
        rejected.insert(source, "https://site.test/a.png", svg);
        assert!(rejected.images.is_empty());
    }
    #[test]
    fn initial_asset_owner_never_follows_replacement_or_opaque_document() {
        let source = "https://site.test/page";
        let target = "https://site.test/a.png";
        let mut images = InitialDocumentImages::default();
        images.insert(source, target, image(target));
        let mut handoff = PromotionImages::new(images);
        handoff.bind(10, source);
        handoff.bind(20, source);
        let mut context = RequestContext::resource(source, Destination::Image, RequestMode::NoCors);
        context.opaque_origin = false;
        assert!(handoff.get(10, source, target, &context, None).is_some());
        assert!(handoff.get(10, source, target, &context, Some(1)).is_none());
        assert!(handoff.get(20, source, target, &context, None).is_none());
        handoff.retire(20);
        assert!(handoff.get(10, source, target, &context, None).is_some());
        context.opaque_origin = true;
        assert!(handoff.get(10, source, target, &context, None).is_none());
        context.opaque_origin = false;
        context.credentialless = true;
        assert!(handoff.get(10, source, target, &context, None).is_none());
        context.credentialless = false;
        assert!(handoff.pending.bytes > 0);
        handoff.retire(10);
        assert_eq!(handoff.pending.bytes, 0);
        assert!(handoff.pending.images.is_empty());
        assert!(handoff.pending.source.is_empty());
        handoff.bind(20, source);
        assert!(handoff.get(10, source, target, &context, None).is_none());
        assert!(handoff.get(20, source, target, &context, None).is_none());
    }
}
