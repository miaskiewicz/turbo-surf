use turbo_dom_parser::rtdom::Tree;
use turbo_surf_view::{aria, locator};

#[test]
fn heading_level_filters_distinguish_every_native_level_and_non_headings() {
    let tree = Tree::parse(
        "<h1 id=a>One</h1><h2 id=b>Two</h2><h3 id=c>Three</h3>\
         <h4 id=d>Four</h4><h5 id=e>Five</h5><h6 id=f>Six</h6>\
         <div id=explicit role=heading aria-level=7>Seven</div><div id=plain>Plain</div>",
    );
    for (id, level) in [
        ("a", 1),
        ("b", 2),
        ("c", 3),
        ("d", 4),
        ("e", 5),
        ("f", 6),
        ("explicit", 7),
    ] {
        let node = tree.get_element_by_id(id).unwrap();
        assert_eq!(aria::heading_level(&tree, node), Some(level));
        assert!(locator::matches_role_options(
            &tree,
            node,
            &locator::LocatorOptions {
                level: Some(level),
                ..Default::default()
            },
        ));
        assert!(!locator::matches_role_options(
            &tree,
            node,
            &locator::LocatorOptions {
                level: Some(level + 1),
                ..Default::default()
            },
        ));
    }
    assert_eq!(
        aria::heading_level(&tree, tree.get_element_by_id("plain").unwrap()),
        None
    );
}

#[test]
fn id_referenced_names_use_the_targets_aria_label_and_image_alternative() {
    let tree = Tree::parse(
        "<span id=label aria-label='  Account   owner '>Ignored text</span>\
         <img id=portrait alt=' Profile   portrait '>\
         <button id=subject aria-labelledby='label portrait'>Ignored button text</button>",
    );
    let subject = tree.get_element_by_id("subject").unwrap();
    assert_eq!(
        aria::accessible_name(&tree, subject),
        "Account owner Profile portrait"
    );
    assert_eq!(
        locator::by_role(
            &tree,
            "button",
            Some(("Account owner Profile portrait", locator::TextMode::Exact))
        ),
        vec![subject],
    );
}

#[test]
fn visibility_hidden_and_collapsed_controls_require_explicit_hidden_inclusion() {
    let tree = Tree::parse(
        "<button id=hidden style='visibility:hidden'>Hidden</button>\
         <button id=collapsed style='visibility:collapse'>Collapsed</button>\
         <button id=visible>Visible</button>",
    );
    for id in ["hidden", "collapsed"] {
        let node = tree.get_element_by_id(id).unwrap();
        assert!(!locator::matches_role_options(
            &tree,
            node,
            &Default::default()
        ));
        assert!(locator::matches_role_options(
            &tree,
            node,
            &locator::LocatorOptions {
                include_hidden: true,
                ..Default::default()
            },
        ));
    }
    assert!(locator::matches_role_options(
        &tree,
        tree.get_element_by_id("visible").unwrap(),
        &Default::default(),
    ));
}
