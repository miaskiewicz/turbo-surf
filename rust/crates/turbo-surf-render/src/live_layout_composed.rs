//! Native composition and selector scope are separate graphs: slot placement
//! changes physical ancestry, while author selectors keep the original DOM. The
//! pure graph is turbo-dom-layout's `cssom_view::composition::Composition`; surf adds
//! the per-scope UA rules and the CSSOM inline-style overrides it layers on top.
use std::collections::HashMap;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

#[derive(Default, Clone)]
pub(crate) struct Composition {
    pub(crate) graph: turbo_dom_layout::cssom_view::composition::Composition,
    pub(crate) ua_rules: Vec<crate::live_stylesheets::ActiveRule>,
    pub(crate) inline_overrides: HashMap<Handle, String>,
}
impl std::ops::Deref for Composition {
    type Target = turbo_dom_layout::cssom_view::composition::Composition;
    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}
impl std::ops::DerefMut for Composition {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}
impl Composition {
    pub(crate) fn scopes(tree: &Tree) -> Result<Self, String> {
        Ok(Self {
            graph: turbo_dom_layout::cssom_view::composition::Composition::scopes(tree)?,
            ..Self::default()
        })
    }
}
