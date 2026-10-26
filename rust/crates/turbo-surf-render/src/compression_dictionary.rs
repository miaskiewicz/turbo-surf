//! Native discovery of compression dictionaries advertised by document responses.
use super::{op2, v8, Base, CspHandle, FrameDocumentUrl, FrameRetired, JsRuntime};
use url::Url;

#[derive(Default)]
pub(super) struct Discovery {
    pending: Vec<String>,
    seen: std::collections::HashSet<String>,
}
impl Discovery {
    pub(super) fn new(base: &str, headers: &[(String, String)]) -> Self {
        Self {
            pending: header_links(base, headers),
            seen: Default::default(),
        }
    }
}

fn context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Context>> {
    if !crate::live_layout_dom::valid_receiver(scope, document.into(), 9)
        || !crate::browser_env::node_has_window_owner(scope, document)
        || !crate::browser_env::is_light_document_node(scope, document)
    {
        return None;
    }
    let context = document.get_creation_context(scope)?;
    if context.get_slot::<FrameRetired>().is_some() {
        return None;
    }
    let caller = scope.get_entered_or_microtask_context();
    caller
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
        .then_some(context)
}

#[op2]
#[serde]
pub(super) fn op_dictionary_headers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Vec<String> {
    context(scope, document)
        .and_then(|context| context.get_slot::<CspHandle>())
        .map_or_else(Vec::new, |handle| {
            std::mem::take(&mut handle.dictionaries.borrow_mut().pending)
        })
}

#[op2(fast)]
pub(super) fn op_dictionary_active<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> bool {
    context(scope, document).is_some()
}

#[op2]
pub(super) fn op_dictionary_links<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    root: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Array>, super::JsErrorBox> {
    let links = if context(scope, document).is_some() {
        crate::compression_dictionary_dom::links(scope, root)
            .map_err(super::JsErrorBox::range_error)?
    } else {
        Vec::new()
    };
    let array = v8::Array::new(scope, links.len() as i32);
    let null = v8::null(scope);
    array.set_prototype(scope, null.into());
    for (index, element) in links.into_iter().enumerate() {
        let key = v8::String::new(scope, &index.to_string()).ok_or_else(|| {
            super::JsErrorBox::generic("Dictionary snapshot key allocation failed")
        })?;
        if array.create_data_property(scope, key.into(), element.into()) != Some(true) {
            return Err(super::JsErrorBox::generic(
                "Dictionary link snapshot unavailable",
            ));
        }
    }
    Ok(array)
}

/// Native policy/URL snapshot; the bootstrap supplies only captured native link
/// attributes and never exposes the request init or dictionary bytes to the page.
#[op2(reentrant)]
#[serde]
pub(super) fn op_dictionary_prepare<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] href: &str,
    #[string] base: &str,
    #[string] relation: &str,
    cancel_id: u32,
) -> Option<(String, String)> {
    if !relation
        .split([' ', '\t', '\n', '\r', '\u{000c}'])
        .any(|token| token.eq_ignore_ascii_case("compression-dictionary"))
    {
        return None;
    }
    let context = context(scope, document)?;
    let handle = context.get_slot::<CspHandle>()?;
    let top_level = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<Base>()
        .0
        .clone();
    let source = context
        .get_slot::<FrameDocumentUrl>()
        .map_or_else(|| top_level.clone(), |url| url.0.clone());
    if Url::parse(&source).ok()?.scheme() != "https" {
        return None;
    }
    let mut target = Url::parse(base).ok()?.join(href).ok()?;
    if target.scheme() != "https" || !target.username().is_empty() || target.password().is_some() {
        return None;
    }
    target.set_fragment(None);
    let target = target.to_string();
    {
        let mut discovery = handle.dictionaries.borrow_mut();
        if discovery.seen.len() >= 64 && !discovery.seen.contains(&target) {
            return None;
        }
        discovery.seen.insert(target.clone());
    }
    if !super::csp_request(scope, document, &target, "connect-src", "") {
        return Some((target, String::new()));
    }
    let init = serde_json::json!({
        "method":"GET", "mode":"cors", "credentials":"omit", "referrerPolicy":"no-referrer",
        "destination":"", "stream":false, "dictionary_prefetch":true, "headers":{"priority":"u=4, i"},
        "source":source, "csp":handle.id, "cancel_id":cancel_id,
        "opaque_origin":super::window_is_opaque(scope, context),
        "top_level_source":top_level,
        "cookie_site_is_opaque":super::frame_cookie_site_is_opaque(scope, context)
    })
    .to_string();
    Some((target, init))
}

fn fields(value: &str, separator: char) -> Vec<&str> {
    let mut result = Vec::new();
    let (mut start, mut quoted, mut escaped, mut target) = (0, false, false, false);
    for (offset, ch) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
        } else if !target && ch == '"' {
            quoted = !quoted;
        } else if !quoted && ch == '<' {
            target = true;
        } else if !quoted && ch == '>' {
            target = false;
        } else if !quoted && !target && ch == separator {
            result.push(&value[start..offset]);
            start = offset + ch.len_utf8();
        }
    }
    if !quoted && !target && !escaped {
        result.push(&value[start..]);
    }
    result
}

fn parameter(value: &str) -> Option<String> {
    let value = value.trim();
    if !value.starts_with('"') {
        return (!value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)))
        .then(|| value.to_owned());
    }
    let mut output = String::new();
    let mut escaped = false;
    let mut chars = value[1..].chars();
    while let Some(ch) = chars.next() {
        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return chars.as_str().trim().is_empty().then_some(output);
        } else if ch.is_ascii_control() && ch != '\t' {
            return None;
        } else {
            output.push(ch);
        }
    }
    None
}

/// Parse Link fields without treating quoted or URI commas as field separators.
/// Admission, CORS, freshness and the response's Use-As-Dictionary remain separate.
pub(crate) fn header_links(base: &str, headers: &[(String, String)]) -> Vec<String> {
    let Ok(base) = Url::parse(base) else {
        return Vec::new();
    };
    if base.scheme() != "https" {
        return Vec::new();
    }
    let mut links = Vec::new();
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("link"))
    {
        for field in fields(value, ',') {
            let Some(field) = field.trim().strip_prefix('<') else {
                continue;
            };
            let Some((target, suffix)) = field.split_once('>') else {
                continue;
            };
            if !suffix.trim().is_empty() && !suffix.trim_start().starts_with(';') {
                continue;
            }
            let (mut relation, mut anchor) = (None, false);
            for part in fields(suffix, ';') {
                let Some((name, value)) = part.trim().split_once('=') else {
                    continue;
                };
                if name.trim().eq_ignore_ascii_case("rel") && relation.is_none() {
                    relation = parameter(value);
                } else if name.trim().eq_ignore_ascii_case("anchor") {
                    anchor = true;
                }
            }
            if anchor
                || !relation.is_some_and(|relation| {
                    relation
                        .split_ascii_whitespace()
                        .any(|token| token.eq_ignore_ascii_case("compression-dictionary"))
                })
            {
                continue;
            }
            let Ok(mut target) = base.join(target) else {
                continue;
            };
            if target.scheme() != "https"
                || !target.username().is_empty()
                || target.password().is_some()
            {
                continue;
            }
            target.set_fragment(None);
            let target = target.to_string();
            if !links.contains(&target) {
                links.push(target);
                if links.len() == 16 {
                    return links;
                }
            }
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    fn links(base: &str, value: &str) -> Vec<String> {
        header_links(base, &[("Link".into(), value.into())])
    }
    #[test]
    fn header_discovery_preserves_uri_and_quoted_delimiters() {
        assert_eq!(
            links(
                "https://site.test/dir/page",
                r#"</a,b>; title="a,b;c"; rel="preload compression-dictionary", <../other>; rel=compression-dictionary"#
            ),
            ["https://site.test/a,b", "https://site.test/other"]
        );
        assert_eq!(
            links(
                "https://site.test/",
                r#"</dict#one>; rel="COMPRESSION-DICTIONARY", </dict#two>; rel=compression-dictionary"#
            ),
            ["https://site.test/dict"]
        );
    }
    #[test]
    fn invalid_or_recontextualized_dictionary_hints_are_not_fetched() {
        for value in [
            r#"</dict>; rel="preload""#,
            r#"</dict>; rel="compression-dictionary"; anchor="/other""#,
            r#"<http://site.test/dict>; rel=compression-dictionary"#,
            r#"<https://user:pass@site.test/dict>; rel=compression-dictionary"#,
            r#"</dict>; rel="compression-dictionary"garbage"#,
            r#"</dict>garbage; rel=compression-dictionary"#,
            r#"</dict>; rel="compression-dictionary"#,
        ] {
            assert!(links("https://site.test/", value).is_empty(), "{value}");
        }
        assert!(links("http://site.test/", "</dict>; rel=compression-dictionary").is_empty());
    }
}
