//! Host-only style authorization keeps authored attributes separate from applied CSS.
use super::*;
use turbo_dom_parser::rtdom::cascade::StylePolicy;

#[derive(Default)]
pub(super) struct PolicyState {
    callback: Option<v8::Weak<v8::Function>>,
    sheets: HashMap<Handle, (String, bool)>,
    inline: HashMap<Handle, Inline>,
    cascade: Option<(u64, StylePolicy)>,
}
#[derive(Clone)]
struct Inline {
    source: String,
    applied: String,
}
fn authorize(
    scope: &mut v8::PinScope,
    node: Handle,
    kind: &str,
    text: &str,
    initial: Option<&[String]>,
) -> bool {
    let callback = DOM.with(|dom| {
        dom.borrow()
            .as_ref()
            .and_then(|dom| dom.css_policy.callback.clone())
    });
    let Some(callback) = callback else {
        return true;
    };
    let callback = dom_ownership::local(scope, callback);
    let root = with_tree(Tree::root).unwrap();
    let document = wrap(scope, root);
    let element = wrap(scope, node);
    let kind = v8::String::new(scope, kind).unwrap();
    let text = v8::String::new(scope, text).unwrap();
    let metas: v8::Local<v8::Value> = if let Some(initial) = initial {
        let array = v8::Array::new(scope, initial.len() as i32);
        for (index, value) in initial.iter().enumerate() {
            let value = v8::String::new(scope, value).unwrap();
            array.set_index(scope, index as u32, value.into());
        }
        array.into()
    } else {
        v8::undefined(scope).into()
    };
    let receiver = v8::undefined(scope);
    callback
        .call(
            scope,
            receiver.into(),
            &[
                document.into(),
                element.into(),
                kind.into(),
                text.into(),
                metas,
            ],
        )
        .is_some_and(|value| value.is_true())
}
fn source(node: Handle, sheet: bool) -> String {
    with_tree(|tree| {
        if sheet {
            tree.text_content(node)
        } else {
            tree.get_attribute(node, "style").unwrap_or("").to_owned()
        }
    })
    .unwrap_or_default()
}
fn inline_changed(scope: &mut v8::PinScope, node: Handle, initial: Option<&[String]>) {
    let source = source(node, false);
    let present = with_tree(|tree| tree.has_attribute(node, "style")).unwrap_or(false);
    let allowed = authorize(scope, node, "style-src-attr", &source, initial);
    DOM.with(|dom| {
        let mut dom = dom.borrow_mut();
        let policy = &mut dom.as_mut().unwrap().css_policy;
        let applied = if !present {
            String::new()
        } else if allowed {
            source.clone()
        } else {
            policy
                .inline
                .get(&node)
                .map(|old| old.applied.clone())
                .unwrap_or_default()
        };
        policy.inline.insert(node, Inline { source, applied });
    });
}
pub(super) fn attribute_changed(scope: &mut v8::PinScope, node: Handle) {
    inline_changed(scope, node, None);
}
fn applied_inline(scope: &mut v8::PinScope, node: Handle) -> String {
    let text = source(node, false);
    let cached = DOM.with(|dom| {
        dom.borrow()
            .as_ref()
            .unwrap()
            .css_policy
            .inline
            .get(&node)
            .cloned()
    });
    if cached.as_ref().is_none_or(|entry| entry.source != text) {
        inline_changed(scope, node, None);
    }
    DOM.with(|dom| {
        dom.borrow()
            .as_ref()
            .unwrap()
            .css_policy
            .inline
            .get(&node)
            .unwrap()
            .applied
            .clone()
    })
}
fn sheet_allowed(scope: &mut v8::PinScope, node: Handle, initial: Option<&[String]>) -> bool {
    let text = source(node, true);
    let cached = DOM.with(|dom| {
        dom.borrow()
            .as_ref()
            .unwrap()
            .css_policy
            .sheets
            .get(&node)
            .cloned()
    });
    if initial.is_none() {
        if let Some((source, allowed)) = cached {
            if source == text {
                return allowed;
            }
        }
    }
    let allowed = authorize(scope, node, "style-src-elem", &text, initial);
    DOM.with(|dom| {
        dom.borrow_mut()
            .as_mut()
            .unwrap()
            .css_policy
            .sheets
            .insert(node, (text, allowed))
    });
    allowed
}
pub(super) fn invalidate_sheet(node: Handle) {
    DOM.with(|dom| {
        if let Some(dom) = dom.borrow_mut().as_mut() {
            dom.css_policy.sheets.remove(&node);
        }
    });
}
pub fn install(scope: &mut v8::PinScope, callback: v8::Local<v8::Function>) {
    let Some(callback) = dom_ownership::helper_function(scope, callback) else {
        return;
    };
    DOM.with(|dom| {
        dom.borrow_mut().as_mut().unwrap().css_policy = PolicyState {
            callback: Some(callback),
            ..Default::default()
        }
    });
    // Native preorder preserves parser policy position. Later meta policies never
    // retroactively revoke a style that was already authorized.
    let nodes = with_tree(|tree| {
        let mut nodes = Vec::new();
        let mut stack = vec![tree.root()];
        while let Some(node) = stack.pop() {
            nodes.push(node);
            let children = tree.children(node);
            stack.extend(children.into_iter().rev());
        }
        nodes
    })
    .unwrap_or_default();
    let mut metas = Vec::new();
    for node in nodes {
        let (tag, style, meta) = with_tree(|tree| {
            (
                tree.local_name(node).unwrap_or("").to_owned(),
                tree.has_attribute(node, "style"),
                if tree.local_name(node) == Some("meta")
                    && tree
                        .parent(node)
                        .is_some_and(|parent| tree.local_name(parent) == Some("head"))
                    && tree
                        .get_attribute(node, "http-equiv")
                        .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
                {
                    tree.get_attribute(node, "content").map(str::to_owned)
                } else {
                    None
                },
            )
        })
        .unwrap();
        if style {
            inline_changed(scope, node, Some(&metas));
        }
        if tag == "style" {
            sheet_allowed(scope, node, Some(&metas));
        }
        if let Some(meta) = meta {
            metas.push(meta);
        }
    }
}
pub(super) fn cascade_policy(scope: &mut v8::PinScope) -> StylePolicy {
    let version = with_tree(|tree| tree.version).unwrap_or_default();
    if let Some(policy) = DOM.with(|dom| {
        let dom = dom.borrow();
        let state = &dom.as_ref().unwrap().css_policy;
        if state.callback.is_none() {
            return Some(StylePolicy::default());
        }
        state
            .cascade
            .as_ref()
            .filter(|(cached, _)| *cached == version)
            .map(|(_, policy)| policy.clone())
    }) {
        return policy;
    }
    let mut policy = StylePolicy::default();
    let nodes = with_tree(|tree| {
        tree.handles()
            .filter(|&node| tree.node_type_id(node) == 1)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for node in nodes {
        if with_tree(|tree| tree.local_name(node) == Some("style")).unwrap_or(false)
            && !sheet_allowed(scope, node, None)
        {
            policy.blocked_stylesheets.insert(node);
        }
        let text = source(node, false);
        let applied = applied_inline(scope, node);
        if text != applied {
            policy.inline_overrides.insert(node, applied);
        }
    }
    DOM.with(|dom| {
        dom.borrow_mut().as_mut().unwrap().css_policy.cascade = Some((version, policy.clone()))
    });
    policy
}
fn read_inline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let _dom = DomScope::for_object(scope, args.this());
    let Some(node) = receiver_handle(scope, &args, &[1]) else {
        return;
    };
    let text = applied_inline(scope, node);
    rv.set(v8::String::new(scope, &text).unwrap().into());
}
fn write_inline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue,
) {
    let _dom = DomScope::for_object(scope, args.this());
    let Some(node) = receiver_handle(scope, &args, &[1]) else {
        return;
    };
    let text = arg_str(scope, &args, 0);
    with_tree_mut(|tree| tree.set_attribute(node, "style", &text));
    DOM.with(|dom| {
        dom.borrow_mut().as_mut().unwrap().css_policy.inline.insert(
            node,
            Inline {
                source: text.clone(),
                applied: text,
            },
        )
    });
}
fn read_sheet(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let _dom = DomScope::for_object(scope, args.this());
    let Some(node) = receiver_handle(scope, &args, &[1]) else {
        return;
    };
    rv.set_bool(sheet_allowed(scope, node, None));
}
pub(super) fn register(scope: &mut v8::PinScope) {
    let global = scope.get_current_context().global(scope);
    bind_method(scope, global, "__readAppliedInlineStyle", read_inline);
    bind_method(scope, global, "__writeAppliedInlineStyle", write_inline);
    bind_method(scope, global, "__isInlineSheetAllowed", read_sheet);
}
impl PolicyState {
    pub(super) fn sweep_storage(&mut self, tree: &Tree) {
        self.sheets.retain(|node, _| tree.is_live(*node));
        self.inline.retain(|node, _| tree.is_live(*node));
        self.cascade = None;
    }
}
