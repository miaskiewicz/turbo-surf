use turbo_surf_render::run_with_dom;

#[test]
fn tree_walker_backward_parent_and_skip_axes() {
    let out = run_with_dom(
        "<body></body>",
        r#"(() => {
          const root = document.createElement('div');
          root.innerHTML = '<section><span id="inside"></span></section><span id="sibling"></span>';
          const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT,
            node => node.nodeName === 'SECTION' ? NodeFilter.FILTER_SKIP : NodeFilter.FILTER_ACCEPT);
          return JSON.stringify([walker.firstChild().id, walker.nextSibling().id,
            walker.previousNode().id, walker.parentNode() === root]);
        })()"#,
    )
    .unwrap();
    assert_eq!(out, r#"["inside","sibling","inside",true]"#);
}

#[test]
fn node_iterator_root_and_reference_direction() {
    let out = run_with_dom(
        "<body></body>",
        r#"(() => {
          const root = document.createElement('div'); root.id = 'root';
          root.innerHTML = '<span id="child"></span>';
          const iterator = document.createNodeIterator(root, NodeFilter.SHOW_ELEMENT);
          return JSON.stringify([iterator.nextNode().id, iterator.nextNode().id,
            iterator.previousNode().id, iterator.previousNode().id,
            iterator.referenceNode === root, iterator.pointerBeforeReferenceNode]);
        })()"#,
    )
    .unwrap();
    assert_eq!(out, r#"["root","child","child","root",true,true]"#);
}

#[test]
fn tree_walker_propagates_filter_exception_and_retains_cursor() {
    let out = run_with_dom(
        "<body></body>",
        r#"(() => {
          const root = document.createElement('div');
          root.append(document.createElement('span'));
          const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT,
            () => {throw new Error('filter-marker');});
          try { walker.nextNode(); return 'resolved'; }
          catch (error) { return JSON.stringify([error.message, walker.currentNode === root]); }
        })()"#,
    )
    .unwrap();
    assert_eq!(out, r#"["filter-marker",true]"#);
}
