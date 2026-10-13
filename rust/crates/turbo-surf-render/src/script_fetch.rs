//! Script fetches validate every CORS response before following its redirect.
use std::collections::BTreeMap;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_html, FetchOptions, FetchResult};
use turbo_surf_core::request_context::{same_origin, ClientHints, RequestContext, RequestMode};

type ScriptPolicy<'a> = &'a dyn Fn(&str, bool) -> bool;

pub async fn fetch(
    url: &str,
    mut context: RequestContext,
    mut headers: BTreeMap<String, String>,
    hints: &ClientHints,
    jar: &mut CookieJar,
    policy: Option<ScriptPolicy<'_>>,
) -> Result<FetchResult, String> {
    let mut target = url.to_string();
    let mut redirected = false;
    for hop in 0..=20 {
        if policy.is_some_and(|allowed| !allowed(&target, redirected)) {
            return Err("Script blocked by Content Security Policy".into());
        }
        let parsed = url::Url::parse(&target).map_err(|error| error.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Script URL is not an allowed HTTP URL".into());
        }
        let source = context.source.as_deref().unwrap_or("");
        let cross_origin =
            context.opaque_origin || context.origin_tainted || !same_origin(source, &target);
        if context.mode == RequestMode::SameOrigin && cross_origin {
            return Err("Script redirect violates same-origin mode".into());
        }
        let mut response = fetch_html(
            &target,
            FetchOptions {
                headers: headers.clone(),
                context: Some(context.clone()),
                client_hints: Some(hints),
                jar: Some(jar),
                allow_non_html: true,
                max_redirects: Some(0),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        if context.mode == RequestMode::Cors
            && cross_origin
            && !crate::fetch_cors::allowed(&context, &response.headers)
        {
            return Err("Script CORS response is not allowed".into());
        }
        let location = matches!(response.status, 301 | 302 | 303 | 307 | 308)
            .then(|| crate::fetch_cors::header(&response.headers, "location"))
            .flatten();
        let Some(location) = location else {
            if context.destination == turbo_surf_core::request_context::Destination::Script
                && nosniff(&response.headers)
                && !javascript_mime(&response.content_type)
            {
                return Err("Script MIME type is blocked by nosniff".into());
            }
            response.redirected = redirected;
            return Ok(response);
        };
        if hop == 20 {
            return Err("Script redirect limit exceeded".into());
        }
        let next = parsed
            .join(location)
            .map_err(|error| error.to_string())?
            .to_string();
        if !same_origin(&target, &next) {
            headers.retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
        }
        context.follow_redirect(&target, &next);
        target = next;
        redirected = true;
    }
    unreachable!("bounded redirect loop always returns")
}

pub fn javascript_mime(content_type: &str) -> bool {
    matches!(
        content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "application/javascript"
            | "text/javascript"
            | "application/ecmascript"
            | "text/ecmascript"
            | "application/x-javascript"
            | "application/x-ecmascript"
            | "text/jscript"
            | "text/livescript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/x-javascript"
            | "text/x-ecmascript"
    )
}

fn nosniff(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-content-type-options"))
        .and_then(|(_, value)| value.split(',').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("nosniff"))
}
