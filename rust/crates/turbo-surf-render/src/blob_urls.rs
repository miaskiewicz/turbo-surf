//! The blob URL store and the `blob:`/`data:` scheme fetches (Chrome 156).
//!
//! `URL.createObjectURL` mints `blob:<origin>/<uuid v4>` into a native store keyed
//! by the creating document's storage key (origin, top-level site, credentialless
//! partition); an opaque origin mints `blob:null/…`, reachable only from the same
//! document. Fetches resolve a URL only under the same key, so a revoked, foreign
//! or cross-partition URL fails like a missing one.
use super::*;

struct Entry {
    key: String,
    bytes: Rc<[u8]>,
    content_type: String,
}

#[derive(Default)]
pub(super) struct Store(std::collections::HashMap<String, Entry>);

/// The storage key a document's blob URLs live under, with the origin they print.
fn storage_key(
    scope: &mut v8::PinScope,
    state: &OpState,
    document: v8::Local<v8::Object>,
) -> Option<(String, String)> {
    let context = document.get_creation_context(scope)?;
    let handle = context.get_slot::<CspHandle>();
    let partition = handle
        .as_ref()
        .and_then(|handle| handle.partition.as_ref())
        .map_or(0, |partition| partition.id);
    let opaque = window_is_opaque(scope, context);
    let origin = if opaque {
        "null".to_string()
    } else {
        window_origin(scope, context)
    };
    // An opaque origin is unique to its document; nothing else shares its key.
    let document_id = if opaque {
        handle.as_ref().map_or(0, |handle| handle.id)
    } else {
        0
    };
    let top = turbo_surf_core::request_context::schemeful_site(&state.borrow::<Base>().0)
        .unwrap_or_default();
    Some((
        format!("{origin}\u{0}{top}\u{0}{partition}\u{0}{document_id}"),
        origin,
    ))
}

fn uuid() -> Result<String, JsErrorBox> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

#[op2]
#[string]
pub(super) fn op_blob_url_create(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    #[buffer] bytes: &[u8],
    #[string] content_type: String,
) -> Result<String, JsErrorBox> {
    let (key, origin) = storage_key(scope, state, document)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let url = format!("blob:{origin}/{}", uuid()?);
    state.borrow_mut::<Store>().0.insert(
        url.clone(),
        Entry {
            key,
            bytes: bytes.into(),
            content_type,
        },
    );
    Ok(url)
}

/// Revocation only reaches URLs of the caller's own storage key.
#[op2(fast)]
pub(super) fn op_blob_url_revoke(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    #[string] url: &str,
) {
    let Some((key, _)) = storage_key(scope, state, document) else {
        return;
    };
    let store = &mut state.borrow_mut::<Store>().0;
    if store.get(url).is_some_and(|entry| entry.key == key) {
        store.remove(url);
    }
}

/// The caller's storage key, sent with a `blob:` fetch.
#[op2]
#[string]
pub(super) fn op_blob_url_key(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> String {
    storage_key(scope, state, document).map_or_else(String::new, |(key, _)| key)
}

/// A live blob URL's bytes for download capture, under the caller's key.
#[op2]
#[buffer]
pub(super) fn op_blob_url_read(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    #[string] url: &str,
) -> Option<Vec<u8>> {
    let (key, _) = storage_key(scope, state, document)?;
    lookup(state, url, &key).map(|(bytes, _)| bytes.to_vec())
}

fn lookup(state: &OpState, url: &str, key: &str) -> Option<(Rc<[u8]>, String)> {
    let url = url.split('#').next().unwrap_or(url);
    state
        .try_borrow::<Store>()?
        .0
        .get(url)
        .filter(|entry| entry.key == key)
        .map(|entry| (entry.bytes.clone(), entry.content_type.clone()))
}

/// A single `bytes=` range against `size`: `None` when absent, `Some(None)` when
/// it cannot be served (multiple ranges, malformed, unsatisfiable).
fn byte_range(header: Option<&str>, size: usize) -> Option<Option<(usize, usize)>> {
    let header = header?.trim();
    let parse = || -> Option<(usize, usize)> {
        let spec = header.strip_prefix("bytes=")?.trim();
        let (start, end) = spec.split_once('-')?;
        let (start, end) = (start.trim(), end.trim());
        if start.is_empty() {
            let suffix = end.parse::<usize>().ok().filter(|suffix| *suffix > 0)?;
            return Some((size.saturating_sub(suffix), size.checked_sub(1)?));
        }
        let start = start.parse::<usize>().ok().filter(|start| *start < size)?;
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<usize>().ok()?.min(size - 1)
        };
        (start <= end).then_some((start, end))
    };
    Some(parse())
}

fn header<'a>(
    headers: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn failure(target: &str) -> FetchOut {
    FetchOut {
        final_url: target.to_string(),
        ..Default::default()
    }
}

/// `blob:` scheme fetch: GET only, the creating storage key only, one byte range.
/// Chrome reports a 206 as "Partial Content" to XMLHttpRequest and "OK" to fetch.
pub(super) fn fetch_blob(
    state: &OpState,
    target: &str,
    method: Option<&str>,
    headers: &std::collections::BTreeMap<String, String>,
    key: &str,
    xhr: bool,
) -> FetchOut {
    let url = target.split('#').next().unwrap_or(target);
    let Some((bytes, content_type)) = lookup(state, url, key) else {
        return failure(url);
    };
    if method.is_some_and(|method| method != "GET") {
        return failure(url);
    }
    let mut response_headers = vec![("content-type".to_string(), content_type.clone())];
    let (status, status_text, body) = match byte_range(header(headers, "range"), bytes.len()) {
        None => (200, "OK", &bytes[..]),
        Some(None) => return failure(url),
        Some(Some((start, end))) => {
            response_headers.push((
                "content-range".into(),
                format!("bytes {start}-{end}/{}", bytes.len()),
            ));
            (
                206,
                if xhr { "Partial Content" } else { "OK" },
                &bytes[start..=end],
            )
        }
    };
    response_headers.push(("content-length".into(), body.len().to_string()));
    FetchOut {
        status,
        status_text: status_text.into(),
        ok: true,
        response_received: true,
        body_bytes: body.to_vec(),
        headers: response_headers,
        content_type,
        final_url: url.to_string(),
        ..Default::default()
    }
}

/// The percent-decoded (and base64-decoded) body of a `data:` URL with its
/// metadata, or `None` for a malformed URL.
pub(super) fn data_url_bytes(url: &str) -> Option<(Vec<u8>, String)> {
    use base64::Engine;
    let url = url.strip_prefix("data:")?.split('#').next()?;
    let (metadata, payload) = url.split_once(',')?;
    let bytes = percent_encoding::percent_decode_str(payload).collect::<Vec<u8>>();
    let trimmed = metadata.trim_end_matches([' ', '\t']);
    let base64 = trimmed
        .rsplit_once(';')
        .is_some_and(|(_, last)| last.trim().eq_ignore_ascii_case("base64"));
    let metadata = if base64 {
        trimmed.rsplit_once(';').map_or("", |(rest, _)| rest)
    } else {
        metadata
    };
    if !base64 {
        return Some((bytes, metadata.to_string()));
    }
    let mut bytes = bytes;
    bytes.retain(|byte| !byte.is_ascii_whitespace());
    let config = base64::engine::general_purpose::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent);
    let bytes =
        base64::engine::general_purpose::GeneralPurpose::new(&base64::alphabet::STANDARD, config)
            .decode(bytes)
            .ok()?;
    Some((bytes, metadata.to_string()))
}

/// A data: URL's MIME type as Chrome serializes it: `text/plain;charset=US-ASCII`
/// when it does not parse, otherwise the lowercased essence and parameter names.
fn data_mime(metadata: &str) -> String {
    let metadata = metadata.trim();
    let metadata = if metadata.starts_with(';') {
        format!("text/plain{metadata}")
    } else {
        metadata.to_string()
    };
    let mut fields = metadata.split(';');
    let essence = fields.next().unwrap_or("").trim();
    let token = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    };
    let Some((kind, subtype)) = essence.split_once('/') else {
        return "text/plain;charset=US-ASCII".into();
    };
    if !token(kind) || !token(subtype) {
        return "text/plain;charset=US-ASCII".into();
    }
    let mut out = essence.to_ascii_lowercase();
    for field in fields {
        let Some((name, value)) = field.split_once('=') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        if token(&name) && !value.is_empty() {
            out.push_str(&format!(";{name}={value}"));
        }
    }
    out
}

/// `data:` scheme fetch for non-navigation destinations: any method, no ranges.
pub(super) fn fetch_data(target: &str) -> FetchOut {
    let url = target.split('#').next().unwrap_or(target);
    let Some((bytes, metadata)) = data_url_bytes(url) else {
        return failure(url);
    };
    let content_type = data_mime(&metadata);
    FetchOut {
        status: 200,
        status_text: "OK".into(),
        ok: true,
        response_received: true,
        body_bytes: bytes,
        headers: vec![("content-type".into(), content_type.clone())],
        content_type,
        final_url: url.to_string(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_follow_chrome() {
        assert_eq!(byte_range(None, 10), None);
        assert_eq!(byte_range(Some("bytes=1-3"), 10), Some(Some((1, 3))));
        assert_eq!(byte_range(Some("bytes=6-"), 10), Some(Some((6, 9))));
        assert_eq!(byte_range(Some("bytes=-4"), 10), Some(Some((6, 9))));
        assert_eq!(byte_range(Some("bytes=-40"), 10), Some(Some((0, 9))));
        assert_eq!(byte_range(Some("bytes=2-40"), 10), Some(Some((2, 9))));
        assert_eq!(byte_range(Some("bytes=20-30"), 10), Some(None));
        assert_eq!(byte_range(Some("bytes=3-1"), 10), Some(None));
        assert_eq!(byte_range(Some("bytes=0-1,3-4"), 10), Some(None));
        assert_eq!(byte_range(Some("items=0-1"), 10), Some(None));
        assert_eq!(byte_range(Some("bytes=-0"), 10), Some(None));
        assert_eq!(byte_range(Some("bytes=0-"), 0), Some(None));
    }

    #[test]
    fn data_mime_serializes_like_chrome() {
        assert_eq!(data_mime(""), "text/plain;charset=US-ASCII");
        assert_eq!(data_mime("text/plain"), "text/plain");
        assert_eq!(
            data_mime("Text/HTML;Charset=utf-8"),
            "text/html;charset=utf-8"
        );
        assert_eq!(data_mime(";charset=utf-8"), "text/plain;charset=utf-8");
        assert_eq!(data_mime("bogus"), "text/plain;charset=US-ASCII");
        assert_eq!(data_mime("a b/c"), "text/plain;charset=US-ASCII");
        assert_eq!(data_mime("text/plain;flag"), "text/plain");
    }

    #[test]
    fn data_urls_decode() {
        assert_eq!(
            data_url_bytes("data:text/plain,a%20b#frag"),
            Some((b"a b".to_vec(), "text/plain".into()))
        );
        assert_eq!(
            data_url_bytes("data:;BASE64,aGk="),
            Some((b"hi".to_vec(), "".into()))
        );
        assert_eq!(data_url_bytes("data:;base64,%%%"), None);
        assert_eq!(data_url_bytes("data:nocomma"), None);
        assert_eq!(
            fetch_data("data:,x").headers[0].1,
            "text/plain;charset=US-ASCII"
        );
        assert_eq!(fetch_data("data:;base64,!").status, 0);
    }
}
