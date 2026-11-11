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
