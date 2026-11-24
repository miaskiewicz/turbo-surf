//! Reuse prepared sheet snapshots only while every selector input remains current.
use crate::live_layout_composed::Composition;
use crate::live_stylesheets::SheetSnapshot;
use deno_core::v8;
use std::cell::RefCell;
use std::rc::Rc;
use turbo_dom_browser_env::host_internals::{
    current_document_context, style_revisions, with_tree, DocumentCheckpoint, Handle,
    StyleRevisions,
};

#[derive(PartialEq)]
enum DocumentInputs {
    Connected(DocumentCheckpoint),
    Storage(u64),
}

pub(crate) struct Key {
    document: u32,
    root: Handle,
    version: u64,
    inputs: DocumentInputs,
    revisions: StyleRevisions,
    composition: Composition,
}

impl Key {
    pub(crate) fn new(
        document: u32,
        root: Handle,
        version: u64,
        checkpoint: Option<DocumentCheckpoint>,
        graph: &Composition,
    ) -> Self {
        Self {
            document,
            root,
            version,
            inputs: checkpoint.map_or(DocumentInputs::Storage(version), DocumentInputs::Connected),
            revisions: style_revisions(),
            composition: graph.clone(),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.document == other.document
            && self.root == other.root
            && self.inputs == other.inputs
            && self.revisions == other.revisions
            && self.composition.children == other.composition.children
            && self.composition.parents == other.composition.parents
            && self.composition.scopes == other.composition.scopes
            && self.composition.hosts == other.composition.hosts
            && self.composition.depths == other.composition.depths
            && self.composition.ua_rules == other.composition.ua_rules
            && self.composition.inline_overrides == other.composition.inline_overrides
    }

    fn still_current(&self, scope: &mut v8::PinScope) -> bool {
        let checkpoint_current = match &self.inputs {
            DocumentInputs::Connected(checkpoint) => {
                crate::live_layout_dom::current_document_checkpoint(scope).as_ref()
                    == Some(checkpoint)
            }
            DocumentInputs::Storage(_) => true,
        };
        checkpoint_current
            && self.revisions == style_revisions()
            && with_tree(|tree| tree.version) == Some(self.version)
    }
}

struct Entry {
    key: Key,
    sheets: Rc<Vec<SheetSnapshot>>,
}

#[derive(Default)]
struct Cache(RefCell<Option<Entry>>);

pub(crate) fn read(scope: &mut v8::PinScope, key: &Key) -> Option<Rc<Vec<SheetSnapshot>>> {
    let context = current_document_context(scope)?;
    let cache = context.get_slot::<Cache>()?;
    let entry = cache.0.borrow();
    let entry = entry.as_ref().filter(|entry| entry.key.matches(key))?;
    Some(entry.sheets.clone())
}

pub(crate) fn store(scope: &mut v8::PinScope, key: Key, sheets: Rc<Vec<SheetSnapshot>>) {
    // A private sheet getter may refresh CSSOM while capturing. Never certify that
    // snapshot against a revision observed only after its inputs were collected.
    if !key.still_current(scope) {
        return;
    }
    let Some(context) = current_document_context(scope) else {
        return;
    };
    if context.get_slot::<Cache>().is_none() {
        context.set_slot(Rc::new(Cache::default()));
    }
    let cache = context.get_slot::<Cache>().unwrap();
    *cache.0.borrow_mut() = Some(Entry { key, sheets });
}
