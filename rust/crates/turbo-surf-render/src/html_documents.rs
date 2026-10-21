//! Native parsed-document default sanitizer. The factual policy snapshot comes
//! from Chrome154's public Sanitizer.get() API; parsing and filtering happen here.
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use turbo_dom_parser::rtdom::tree::Tree;

struct Policy {
    global: HashSet<String>,
    elements: HashMap<(String, String), HashSet<String>>,
}
fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("browser_sanitizer_default.json"))
                .expect("recorded default sanitizer policy");
        let config = &data["configuration"];
        let names = |values: &serde_json::Value| {
            values
                .as_array()
                .unwrap()
                .iter()
                .filter(|attr| attr["namespace"].is_null())
                .map(|attr| attr["name"].as_str().unwrap().to_string())
                .collect::<HashSet<_>>()
        };
        Policy {
            global: names(&config["attributes"]),
            elements: config["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|element| {
                    (
                        (
                            element["namespace"].as_str().unwrap().to_string(),
                            element["name"].as_str().unwrap().to_string(),
                        ),
                        names(&element["attributes"]),
                    )
                })
                .collect(),
        }
    })
}
fn executable_url(value: &str) -> bool {
    // HTML character references were decoded by the actual parser. URL parsing
    // strips ASCII tab/newline characters and leading C0 control/space characters.
    let value = value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>();
    value
        .trim_start_matches(|c: char| c <= ' ')
        .split(':')
        .next()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("javascript"))
}
pub(super) fn sanitize(tree: &mut Tree) {
    let policy = policy();
    let mut nodes = tree.children(tree.root());
    while let Some(node) = nodes.pop() {
        let kind = tree.node_type_id(node);
        if kind == 8 || kind == 7 {
            if let Some(parent) = tree.parent(node) {
                tree.remove_child(parent, node);
            }
            continue;
        }
        if kind == 1 {
            let key = (
                tree.namespace_uri(node).unwrap_or("").to_owned(),
                tree.local_name(node).unwrap_or_default().to_string(),
            );
            let Some(allowed) = policy.elements.get(&key) else {
                if let Some(parent) = tree.parent(node) {
                    tree.remove_child(parent, node);
                }
                continue;
            };
            let mut namespaced = Vec::new();
            tree.for_each_attr_full(node, |prefix, name, _| {
                if !prefix.is_empty() {
                    namespaced.push(name.to_string());
                }
            });
            for name in namespaced {
                tree.remove_attribute(node, &name);
            }
            for (name, value) in tree.attributes(node) {
                if !(policy.global.contains(&name) || allowed.contains(&name))
                    || (matches!(
                        name.as_str(),
                        "href"
                            | "cite"
                            | "action"
                            | "formaction"
                            | "src"
                            | "poster"
                            | "data"
                            | "background"
                            | "longdesc"
                    ) && executable_url(&value))
                {
                    tree.remove_attribute(node, &name);
                }
            }
        }
        nodes.extend(tree.children(node));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_policy_filters_parsed_namespaces_attributes_and_script_urls() {
        let mut tree=Tree::parse("<html><head><title>safe</title><script>evil()</script></head><body><p id='drop' onclick='evil()'><a href='&#x6a;av&#x09;ascript:evil()'>link</a></p><iframe src='about:blank'></iframe><img src='data:image/png,x'><style>p{color:red}</style><svg onload='evil()'><a href='javascript:evil()'>vector</a></svg><math><mi onclick='evil()'>x</mi></math><!--drop--><unknown><b>drop</b></unknown><template><script>drop</script></template></body></html>");
        sanitize(&mut tree);
        let html = turbo_dom_parser::rtdom::serialize::serialize_inner(&tree, tree.root());
        assert!(!html.contains("script"));
        assert!(!html.contains("iframe"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("<style"));
        assert!(!html.contains("drop"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("onload"));
        assert!(!html.contains("javascript"));
        assert!(html.contains("<title>safe</title>"), "{html}");
        assert!(html.contains("<p><a>link</a></p>"));
        assert!(html.contains("<svg><a>vector</a></svg>"));
        assert!(html.contains("<math><mi>x</mi></math>"));
    }
}
