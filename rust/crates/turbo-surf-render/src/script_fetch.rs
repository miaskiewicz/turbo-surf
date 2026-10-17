//! Script fetches validate every CORS response before following its redirect.
use std::collections::BTreeMap;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_html, FetchOptions, FetchResult};
use turbo_surf_core::request_context::{same_origin, ClientHints, RequestContext, RequestMode};

#[derive(Default)]
pub struct ScriptPolicies<'a> {
    pub request: Option<ScriptPolicy<'a>>,
    pub response_side_effects: Option<&'a turbo_surf_core::net::ResponseSideEffects<'a>>,
    pub http_cache: Option<&'a turbo_surf_core::http_cache::HttpCache>,
    pub cache_partition: &'a str,
    pub embedder: crate::resource_policy::EmbedderPolicy,
}

type ScriptPolicy<'a> = &'a dyn Fn(&str, bool) -> bool;

pub struct ScriptResponse {
    pub response: FetchResult,
    pub muted_errors: bool,
}

pub async fn fetch(
    url: &str,
    mut context: RequestContext,
    mut headers: BTreeMap<String, String>,
    hints: &ClientHints,
    jar: &mut CookieJar,
    policies: ScriptPolicies<'_>,
    integrity: &str,
) -> Result<ScriptResponse, String> {
    let mut target = url.to_string();
    let mut redirected = false;
    for hop in 0..=20 {
        if !crate::resource_policy::active_mixed_content_allowed(
            context.source.as_deref().unwrap_or(""),
            &target,
        ) {
            return Err("Script request blocked as active mixed content".into());
        }
        if policies
            .request
            .is_some_and(|allowed| !allowed(&target, redirected))
        {
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
        let response_policy = |context: &RequestContext,
                               target: &str,
                               headers: &[(String, String)]| {
            crate::resource_policy::embedder_allowed(policies.embedder, context, target, headers)
                && (context.mode != RequestMode::Cors
                    || !(context.opaque_origin
                        || context.origin_tainted
                        || !same_origin(context.source.as_deref().unwrap_or(""), target))
                    || crate::fetch_cors::allowed(context, headers))
        };
        let mut response = fetch_html(
            &target,
            FetchOptions {
                http_cache: policies.http_cache,
                response_policy: Some(&response_policy),
                response_side_effects: policies.response_side_effects,
                cache_partition: policies.cache_partition.into(),
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
        if !crate::resource_policy::embedder_allowed(
            policies.embedder,
            &context,
            &target,
            &response.headers,
        ) {
            return Err("Script response blocked by Cross-Origin-Resource-Policy".into());
        }
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
            if !crate::integrity::matches(
                &response.body_bytes,
                integrity,
                context.mode == RequestMode::Cors || !cross_origin,
            ) {
                return Err("Script response failed integrity validation".into());
            }
            if context.destination == turbo_surf_core::request_context::Destination::Script
                && nosniff(&response.headers)
                && !javascript_mime(&response.content_type)
            {
                return Err("Script MIME type is blocked by nosniff".into());
            }
            response.redirected = redirected;
            return Ok(ScriptResponse {
                response,
                muted_errors: context.mode == RequestMode::NoCors && cross_origin,
            });
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
        context.apply_referrer_policy_headers(&response.headers);
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
