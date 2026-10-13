//! Fetch's CORS response/preflight checks, shared by the native fetch op.
use std::collections::BTreeMap;
use turbo_surf_core::request_context::{origin, Credentials, RequestContext};

pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let mut values = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name));
    let value = values.next()?.1.as_str();
    // Duplicate allow-origin/credentials headers do not form a valid CORS value.
    values.next().is_none().then_some(value.trim())
}

pub fn allowed(context: &RequestContext, headers: &[(String, String)]) -> bool {
    let Some(allow_origin) = header(headers, "access-control-allow-origin") else {
        return false;
    };
    let source = if context.opaque_origin || context.origin_tainted {
        "null".to_string()
    } else {
        context
            .source
            .as_deref()
            .and_then(origin)
            .unwrap_or_else(|| "null".into())
    };
    if allow_origin == "*" && context.credentials != Credentials::Include {
        return true;
    }
    allow_origin == source
        && (context.credentials != Credentials::Include
            || header(headers, "access-control-allow-credentials") == Some("true"))
}

/// Filter network headers in native code, before guest-provided prototypes run.
pub fn exposed_headers(
    context: &RequestContext,
    headers: Vec<(String, String)>,
    cross_origin: bool,
) -> Vec<(String, String)> {
    let exposed: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("access-control-expose-headers"))
        .flat_map(|(_, value)| {
            value
                .split(',')
                .map(|name| name.trim().to_ascii_lowercase())
        })
        .collect();
    let all = context.credentials != Credentials::Include && exposed.iter().any(|name| name == "*");
    headers
        .into_iter()
        .filter(|(name, _)| {
            let name = name.to_ascii_lowercase();
            !matches!(name.as_str(), "set-cookie" | "set-cookie2")
                && (context.mode != turbo_surf_core::request_context::RequestMode::Cors
                    || !cross_origin
                    || all
                    || matches!(
                        name.as_str(),
                        "cache-control"
                            | "content-language"
                            | "content-length"
                            | "content-type"
                            | "expires"
                            | "last-modified"
                            | "pragma"
                    )
                    || exposed.contains(&name))
        })
        .collect()
}

fn safelisted(name: &str, value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    match name {
        "accept" => !value.bytes().any(unsafe_byte),
        "accept-language" | "content-language" => value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" *,-.;=".contains(&byte)),
        "content-type" => {
            !value.bytes().any(unsafe_byte)
                && matches!(
                    value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase()
                        .as_str(),
                    "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
                )
        }
        "range" => value
            .strip_prefix("bytes=")
            .and_then(|range| range.split_once('-'))
            .is_some_and(|(start, end)| {
                !start.is_empty()
                    && start.bytes().all(|byte| byte.is_ascii_digit())
                    && end.bytes().all(|byte| byte.is_ascii_digit())
                    && (end.is_empty()
                        || start
                            .parse::<u64>()
                            .ok()
                            .zip(end.parse::<u64>().ok())
                            .is_some_and(|(start, end)| start <= end))
            }),
        _ => false,
    }
}

fn unsafe_byte(byte: u8) -> bool {
    (byte < 0x20 && byte != b'\t') || byte == 0x7f || b"\"():<>?@[\\]{}".contains(&byte)
}

pub fn unsafe_headers(headers: &BTreeMap<String, String>) -> Vec<String> {
    let mut names: Vec<_> = headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.to_ascii_lowercase();
            (!safelisted(&name, value)).then_some(name)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn preflight_allowed(
    context: &RequestContext,
    method: &str,
    names: &[String],
    headers: &[(String, String)],
) -> bool {
    if !allowed(context, headers) {
        return false;
    }
    let wildcard = context.credentials != Credentials::Include;
    let methods = header(headers, "access-control-allow-methods").unwrap_or("");
    if !matches!(method, "GET" | "HEAD" | "POST")
        && !methods
            .split(',')
            .any(|value| value.trim() == method || wildcard && value.trim() == "*")
    {
        return false;
    }
    let allowed = header(headers, "access-control-allow-headers").unwrap_or("");
    names.iter().all(|name| {
        allowed.split(',').any(|value| {
            value.trim().eq_ignore_ascii_case(name)
                || wildcard && name != "authorization" && value.trim() == "*"
        })
    })
}

/// Negotiate the actual rewritten hop before any unsafe cross-origin request.
pub async fn authorize_request(
    context: RequestContext,
    target: String,
    method: String,
    mut headers: BTreeMap<String, String>,
    hints: turbo_surf_core::request_context::ClientHints,
) -> Result<(), turbo_surf_core::net::HttpError> {
    use turbo_surf_core::net::{fetch_html, ErrorCode, FetchOptions, HttpError};
    use turbo_surf_core::request_context::{same_origin, RequestMode};
    let cross_origin = context.opaque_origin
        || context.origin_tainted
        || context
            .source
            .as_deref()
            .is_none_or(|source| !same_origin(source, &target));
    if context.mode != RequestMode::Cors || !cross_origin {
        return Ok(());
    }
    // The host inserts this identity after stripping forbidden guest headers.
    headers.retain(|name, _| !name.eq_ignore_ascii_case("user-agent"));
    let names = unsafe_headers(&headers);
    if matches!(method.as_str(), "GET" | "HEAD" | "POST") && names.is_empty() {
        return Ok(());
    }
    let mut preflight_context = context.clone();
    preflight_context.credentials = Credentials::Omit;
    let mut preflight_headers =
        BTreeMap::from([("access-control-request-method".into(), method.clone())]);
    if !names.is_empty() {
        preflight_headers.insert("access-control-request-headers".into(), names.join(","));
    }
    let response = fetch_html(
        &target,
        FetchOptions {
            method: Some("OPTIONS".into()),
            headers: preflight_headers,
            context: Some(preflight_context),
            client_hints: Some(&hints),
            allow_non_html: true,
            max_redirects: Some(0),
            ..Default::default()
        },
    )
    .await?;
    if (200..300).contains(&response.status)
        && preflight_allowed(&context, &method, &names, &response.headers)
    {
        Ok(())
    } else {
        Err(HttpError {
            message: "Request blocked by CORS preflight".into(),
            code: ErrorCode::Network,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbo_surf_core::request_context::{Destination, RequestMode};
    #[test]
    fn credentialed_cors_rejects_wildcards_and_duplicate_authorizations() {
        let mut context = RequestContext::resource(
            "https://source.test/page",
            Destination::Empty,
            RequestMode::Cors,
        );
        let wildcard = vec![("access-control-allow-origin".into(), "*".into())];
        assert!(allowed(&context, &wildcard));
        context.credentials = Credentials::Include;
        assert!(!allowed(&context, &wildcard));
        let mut exact = vec![
            (
                "access-control-allow-origin".into(),
                "https://source.test".into(),
            ),
            ("access-control-allow-credentials".into(), "true".into()),
        ];
        assert!(allowed(&context, &exact));
        exact.push((
            "Access-Control-Allow-Origin".into(),
            "https://source.test".into(),
        ));
        assert!(!allowed(&context, &exact));
    }
    #[test]
    fn preflight_checks_methods_names_and_opaque_origins() {
        let mut context = RequestContext::resource(
            "https://source.test/page",
            Destination::Empty,
            RequestMode::Cors,
        );
        context.opaque_origin = true;
        let headers = vec![
            ("access-control-allow-origin".into(), "null".into()),
            ("access-control-allow-methods".into(), "PUT".into()),
            ("access-control-allow-headers".into(), "X-Token".into()),
        ];
        assert!(preflight_allowed(
            &context,
            "PUT",
            &["x-token".into()],
            &headers
        ));
        assert!(!preflight_allowed(&context, "DELETE", &[], &headers));
        assert!(!preflight_allowed(
            &context,
            "PUT",
            &["authorization".into()],
            &headers
        ));
        assert_eq!(
            unsafe_headers(&BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "*/*".into())
            ])),
            vec!["content-type"]
        );
    }
}
