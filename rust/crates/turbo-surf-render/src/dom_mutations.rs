//! Pre-insertion validation for every guest-accessible native mutation seam.
use super::*;

/// Validate the complete result before detaching any existing node. In particular,
/// rtdom's ordinary-node insertion primitives do not themselves reject cycles.
struct Plan {
    nodes: Vec<Handle>,
    remove: Vec<Handle>,
    reference: Option<Handle>,
}

fn plan(
    tree: &Tree,
    parent: Handle,
    incoming: &[Handle],
    reference: Option<Handle>,
    remove: &[Handle],
) -> Result<Plan, &'static str> {
    if !matches!(tree.node_type_id(parent), 1 | 9 | 11) {
        return Err("HierarchyRequestError");
    }
    if reference.is_some_and(|node| tree.parent(node) != Some(parent))
        || remove.iter().any(|&node| tree.parent(node) != Some(parent))
    {
        return Err("NotFoundError");
    }
    let mut nodes = Vec::new();
    for &node in incoming {
        if tree.is_shadow_root(node)
            || !matches!(tree.node_type_id(node), 1 | 3 | 4 | 7 | 8 | 10 | 11)
        {
            return Err("HierarchyRequestError");
        }
        let mut ancestor = Some(parent);
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = ancestor {
            if current == node || !visited.insert(current) {
                return Err("HierarchyRequestError");
            }
            ancestor = tree.parent(current).or_else(|| tree.shadow_host(current));
        }
        let expanded = if tree.node_type_id(node) == 11 {
            tree.children(node)
        } else {
            vec![node]
        };
        for child in expanded {
            // A repeated argument moves its earlier occurrence to the last position.
            nodes.retain(|&existing| existing != child);
            nodes.push(child);
        }
    }
    let original = tree.children(parent);
    let retained = |node: &Handle| !nodes.contains(node) && !remove.contains(node);
    let reference = reference.and_then(|reference| {
        original
            .iter()
            .position(|&node| node == reference)
            .and_then(|index| {
                original[index..]
                    .iter()
                    .find(|node| retained(node))
                    .copied()
            })
    });
    let mut final_children: Vec<_> = original.into_iter().filter(retained).collect();
    let index = reference
        .and_then(|reference| final_children.iter().position(|&node| node == reference))
        .unwrap_or(final_children.len());
    final_children.splice(index..index, nodes.iter().copied());
    if tree.node_type_id(parent) == 9 {
        let mut element = false;
        let mut doctype = false;
        for &node in &final_children {
            match tree.node_type_id(node) {
                1 if !element => element = true,
                10 if !doctype && !element => doctype = true,
                7 | 8 => (),
                _ => return Err("HierarchyRequestError"),
            }
        }
    } else if nodes.iter().any(|&node| tree.node_type_id(node) == 10) {
        return Err("HierarchyRequestError");
    }
    Ok(Plan {
        nodes,
        remove: remove.to_vec(),
        reference,
    })
}

pub(super) fn insert(
    scope: &mut v8::PinScope,
    parent: Handle,
    incoming: &[Handle],
    reference: Option<Handle>,
    remove: &[Handle],
) -> bool {
    let Some(result) = with_tree(|tree| plan(tree, parent, incoming, reference, remove)) else {
        return false;
    };
    let plan = match result {
        Ok(plan) => plan,
        Err(name) => {
            throw_dom_error(
                scope,
                name,
                "The requested mutation would create an invalid DOM tree",
            );
            return false;
        }
    };
    with_tree_mut(|tree| {
        for node in plan.remove {
            if !plan.nodes.contains(&node) {
                tree.remove_child(parent, node);
            }
        }
        for node in plan.nodes {
            tree.insert_before(parent, node, plan.reference);
        }
    });
    true
}

pub(super) fn node_argument(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> Option<Handle> {
    let value = args.get(index);
    if !value.is_object()
        || value
            .to_object(scope)
            .and_then(|object| handle_of(scope, object))
            .is_none()
    {
        let message = v8::String::new(scope, "The argument must be a native Node").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return None;
    }
    arg_handle(scope, args, index)
}

pub(super) fn sibling_mutation(
    scope: &mut v8::PinScope,
    receiver: Handle,
    nodes: &[Handle],
    after: bool,
    replace: bool,
) {
    let Some((parent, reference)) = with_tree(|tree| {
        let parent = tree.parent(receiver)?;
        let reference = if after {
            tree.next_sibling(receiver)
        } else {
            Some(receiver)
        };
        Some((parent, reference))
    })
    .flatten() else {
        return;
    };
    let removed = if replace { vec![receiver] } else { Vec::new() };
    insert(scope, parent, nodes, reference, &removed);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(source: &str) {
        super::super::tests::init_v8();
        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        install_with_html(
            scope,
            "<!doctype html><html><head></head><body></body></html>",
        );
        let scope = std::pin::pin!(v8::TryCatch::new(scope));
        let scope = &mut scope.init();
        let source = v8::String::new(scope, source).unwrap();
        let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
        if result.is_none() {
            panic!(
                "native mutation fixture: {:?}",
                scope
                    .exception()
                    .map(|error| error.to_rust_string_lossy(scope))
            );
        }
        assert!(result.unwrap().is_true());
        reset();
    }
    #[test]
    fn ordinary_self_append_throws_before_parent_links_change() {
        fixture(
            r#"(()=>{const element=document.createElement('div');let error;try{element.appendChild(element)}catch(caught){error=caught.name;}if(error!=='HierarchyRequestError')throw new Error('self append was accepted');if(element.parentNode!==null||element.childNodes.length!==0)throw new Error('self append changed the tree');return true;})()"#,
        );
    }
    #[test]
    fn every_existing_node_insertion_rejects_ancestor_cycles_atomically() {
        fixture(
            r#"(()=>{
          const methods=[(p,c)=>c.appendChild(p),(p,c)=>c.insertBefore(p,null),
            (p,c)=>c.append('prefix',p),(p,c)=>c.prepend(p,'suffix'),
            (p,c)=>c.replaceChildren('prefix',p),(p,c)=>c.before('prefix',p),
            (p,c)=>c.after(p),(p,c)=>c.replaceWith(p),
            (p,c)=>c.insertAdjacentElement('beforebegin',p),
            (p,c)=>c.insertAdjacentElement('afterend',p),
            (p,c)=>c.insertAdjacentElement('afterbegin',p),
            (p,c)=>c.insertAdjacentElement('beforeend',p),
            (p,c)=>p.replaceChild(p,c)];
          for(let index=0;index<methods.length;index++){
            const parent=document.createElement('section'),child=document.createElement('div');
            parent.appendChild(child);child.textContent='original';
            const before=parent.outerHTML;let error;
            try{methods[index](parent,child)}catch(caught){error=caught.name;}
            if(error!=='HierarchyRequestError')throw new Error('cycle seam '+index+' '+error);
            if(parent.outerHTML!==before||parent.parentNode!==null||child.parentNode!==parent)
              throw new Error('non-atomic cycle seam '+index);
          }
          const host=document.createElement('div'),shadow=host.attachShadow({mode:'open'}),inside=document.createElement('span');
          shadow.appendChild(inside);let error;try{inside.appendChild(host)}catch(caught){error=caught.name;}
          if(error!=='HierarchyRequestError'||host.parentNode!==null||inside.parentNode!==shadow)
            throw new Error('shadow-including ancestor cycle');
          return true;
        })()"#,
        );
    }

    #[test]
    fn document_hierarchy_and_fragment_failures_preserve_existing_nodes() {
        fixture(
            r#"(()=>{
          const root=document.documentElement,doctype=document.doctype;
          const reject=(run)=>{const before=Array.from(document.childNodes);let error;
            try{run()}catch(caught){error=caught.name;}
            if(error!=='HierarchyRequestError'||before.length!==document.childNodes.length||before.some((node,index)=>node!==document.childNodes[index]))throw new Error('invalid document mutation was not atomic '+error);};
          const extra=document.createElement('extra');reject(()=>document.appendChild(extra));
          if(extra.parentNode!==null)throw new Error('extra root moved');
          reject(()=>document.appendChild(document.createTextNode('text')));
          reject(()=>document.insertBefore(root,doctype));
          const fragment=document.createDocumentFragment(),first=document.createElement('first'),second=document.createElement('second');
          fragment.append(first,second);reject(()=>document.replaceChild(fragment,root));
          if(fragment.childNodes.length!==2||first.parentNode!==fragment||second.parentNode!==fragment)throw new Error('failed fragment drained');
          const next=document.createElement('html');
          if(document.replaceChild(next,root)!==root||document.documentElement!==next||root.parentNode!==null||document.doctype!==doctype)throw new Error('document root replacement');
          if(document.insertBefore(next,next)!==next||document.documentElement!==next)throw new Error('self reference');
          next.appendChild(root);let error;try{next.appendChild(doctype)}catch(caught){error=caught.name;}
          if(error!=='HierarchyRequestError'||doctype.parentNode!==document)throw new Error('doctype element child');
          return true;
        })()"#,
        );
    }

    #[test]
    fn native_node_brands_references_and_removal_are_checked_before_mutation() {
        fixture(
            r#"(()=>{
          const parent=document.createElement('div'),child=document.createElement('span'),foreign=document.createElement('b');parent.appendChild(child);
          const reject=(name,run)=>{let error;try{run()}catch(caught){error=caught.name;}
            if(error!==name||parent.firstChild!==child||child.parentNode!==parent||foreign.parentNode!==null)throw new Error('invalid argument changed tree '+error);};
          for(const value of [null,undefined,42,'text',{},Object.create(Node.prototype)]){
            reject('TypeError',()=>parent.appendChild(value));reject('TypeError',()=>parent.insertBefore(value,null));reject('TypeError',()=>parent.replaceChild(value,child));}
          reject('NotFoundError',()=>parent.insertBefore(foreign,foreign));
          reject('NotFoundError',()=>parent.replaceChild(foreign,foreign));
          reject('NotFoundError',()=>parent.removeChild(foreign));
          reject('TypeError',()=>parent.insertBefore(foreign));
          reject('TypeError',()=>parent.append('prefix',Symbol('invalid')));
          reject('TypeError',()=>parent.insertAdjacentElement('beforeend',document.createTextNode('text')));
          reject('SyntaxError',()=>parent.insertAdjacentElement('invalid',foreign));
          reject('SyntaxError',()=>parent.insertAdjacentHTML('invalid','<i>invalid</i>'));
          return true;
        })()"#,
        );
    }

    #[test]
    fn valid_moves_fragments_strings_and_repeated_arguments_keep_specified_order() {
        fixture(
            r#"(()=>{
          const parent=document.createElement('div'),a=document.createElement('a'),b=document.createElement('b'),c=document.createElement('i');
          parent.append(a,b,c);parent.insertBefore(b,b);
          if(parent.children[0]!==a||parent.children[1]!==b||parent.children[2]!==c)throw new Error('insert before self reordered');
          parent.prepend('head',c);parent.append('tail');
          if(parent.firstChild.data!=='head'||parent.childNodes[1]!==c||parent.lastChild.data!=='tail')throw new Error('prepend/string order');
          parent.replaceChildren(a,b,a);
          if(parent.children.length!==2||parent.children[0]!==b||parent.children[1]!==a)throw new Error('duplicate order');
          a.before(a,'before');a.after(a,'after');
          if(parent.innerHTML!=='<b></b><a></a>afterbefore')throw new Error('viable sibling '+parent.innerHTML);
          a.replaceWith(a);if(a.parentNode!==parent)throw new Error('self replacement detached');
          const fragment=document.createDocumentFragment();fragment.append('one',c,'two');
          parent.replaceChild(fragment,a);
          if(fragment.childNodes.length!==0||parent.innerHTML!=='<b></b>one<i></i>twoafterbefore'||a.parentNode!==null)throw new Error('fragment replace');
          const detached=document.createElement('aside');if(detached.insertAdjacentElement('beforebegin',a)!==null||a.parentNode!==null)throw new Error('detached adjacent result');
          let error;try{document.documentElement.insertAdjacentHTML('beforebegin','<x></x>')}catch(caught){error=caught.name;}
          if(error!=='NoModificationAllowedError')throw new Error('document adjacentHTML policy');
          return true;
        })()"#,
        );
    }

    #[test]
    fn native_errors_retain_dom_exception_brands_despite_guest_constructor_overrides() {
        fixture(
            r#"(()=>{
          const Original=DOMException,originalString=String,get=WeakMap.prototype.get,set=WeakMap.prototype.set;
          const element=document.createElement('div');let error;
          try{
            globalThis.DOMException=function(){throw new Error('guest constructor invoked')};
            globalThis.String=function(){throw new Error('guest string invoked')};
            WeakMap.prototype.get=function(){throw new Error('guest weak get invoked')};
            WeakMap.prototype.set=function(){throw new Error('guest weak set invoked')};
            try{element.appendChild(element)}catch(caught){error=caught;}
            if(!(error instanceof Original)||error.name!=='HierarchyRequestError'||error.code!==3)throw new Error('unbranded native exception');
          }finally{globalThis.DOMException=Original;globalThis.String=originalString;WeakMap.prototype.get=get;WeakMap.prototype.set=set;}
          if(element.parentNode!==null||element.childNodes.length!==0||Node.prototype.replaceChild.length!==2)throw new Error('mutation or method shape');
          return true;
        })()"#,
        );
    }
}
