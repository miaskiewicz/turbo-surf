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
        // Fetch "scheme fetch": a data: URL is a basic (same-origin) response in
        // every request mode; a redirect to one is a network error.
        if parsed.scheme() == "data" && !redirected {
            return data_response(parsed, integrity);
        }
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
        // fetch_html applied `response_policy` (CORP/COEP and CORS) to this hop's
        // response, cache hits included, before returning it.
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

/// A data: script through the Fetch data: URL processor: its MIME type is the
/// response Content-Type, its charset parameter (else UTF-8, BOM first) decodes the
/// source, integrity applies and errors are not muted.
fn data_response(mut url: url::Url, integrity: &str) -> Result<ScriptResponse, String> {
    url.set_fragment(None);
    let data = turbo_dom_layout::image::decode_data_url(url.as_str())
        .ok_or("Script data: URL is not valid")?;
    if !crate::integrity::matches(&data.bytes, integrity, true) {
        return Err("Script response failed integrity validation".into());
    }
    let encoding = data
        .mime
        .split(';')
        .skip(1)
        .filter_map(|parameter| parameter.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
        .and_then(|(_, label)| encoding_rs::Encoding::for_label(label.trim().as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let html = encoding.decode(&data.bytes).0.into_owned();
    Ok(ScriptResponse {
        response: FetchResult {
            html,
            body_bytes: data.bytes,
            headers: vec![("content-type".into(), data.mime.clone())],
            final_url: url.into(),
            status: 200,
            status_text: "OK".into(),
            redirected: false,
            content_type: data.mime,
        },
        muted_errors: false,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use turbo_surf_core::request_context::Destination;

    /// Serves `/js` (script), `/loop` (redirects to itself), `/corp` (CORP same-origin)
    /// and `/plain` (no CORS headers).
    async fn server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                        let mut chunk = [0; 1024];
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(count) => request.extend_from_slice(&chunk[..count]),
                        }
                    }
                    let request = String::from_utf8_lossy(&request);
                    let path = request.split_whitespace().nth(1).unwrap_or("/");
                    let (status, extra) = match path {
                        "/loop" => ("302 Found", "Location: /loop\r\n"),
                        "/todata" => ("302 Found", "Location: data:,x\r\n"),
                        "/corp" => ("200 OK", "Cross-Origin-Resource-Policy: same-origin\r\n"),
                        _ => ("200 OK", ""),
                    };
                    let body = "globalThis.loaded=true;";
                    let response = format!("HTTP/1.1 {status}\r\n{extra}Content-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        origin
    }

    fn context(source: &str, mode: RequestMode) -> RequestContext {
        let mut context = RequestContext::navigation(Some(source.into()), false);
        context.destination = Destination::Script;
        context.mode = mode;
        context
    }

    async fn run(
        url: &str,
        context: RequestContext,
        policies: ScriptPolicies<'_>,
    ) -> Result<ScriptResponse, String> {
        let mut jar = CookieJar::new();
        fetch(
            url,
            context,
            BTreeMap::new(),
            &ClientHints::default(),
            &mut jar,
            policies,
            "",
        )
        .await
    }

    async fn error(url: &str, context: RequestContext, policies: ScriptPolicies<'_>) -> String {
        match run(url, context, policies).await {
            Ok(_) => panic!("{url} was admitted"),
            Err(error) => error,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_side_policies_block_before_any_network_io() {
        let origin = server().await;
        let same = format!("{origin}/js");
        // A loopback http URL is potentially trustworthy, so use a public host.
        assert_eq!(
            error(
                "http://script.test/x.js",
                context("https://page.test/", RequestMode::NoCors),
                Default::default()
            )
            .await,
            "Script request blocked as active mixed content"
        );
        let deny = |_: &str, _: bool| false;
        let csp = ScriptPolicies {
            request: Some(&deny),
            ..Default::default()
        };
        assert_eq!(
            error(&same, context(&origin, RequestMode::NoCors), csp).await,
            "Script blocked by Content Security Policy"
        );
        for url in ["ftp://127.0.0.1/x.js", "http://user:pw@127.0.0.1/x.js"] {
            assert_eq!(
                error(
                    url,
                    context(&origin, RequestMode::NoCors),
                    Default::default()
                )
                .await,
                "Script URL is not an allowed HTTP URL"
            );
        }
        let cross = same.replace("127.0.0.1", "localhost");
        assert_eq!(
            error(
                &cross,
                context(&origin, RequestMode::SameOrigin),
                Default::default()
            )
            .await,
            "Script redirect violates same-origin mode"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_policies_and_the_redirect_limit_reject_the_script() {
        let origin = server().await;
        let foreign = "http://foreign.test/";
        let corp = format!("{origin}/corp");
        // CORP and CORS are enforced as the fetch's native response policy.
        assert_eq!(
            error(
                &corp,
                context(foreign, RequestMode::NoCors),
                Default::default()
            )
            .await,
            "Network: Response blocked by native document policy"
        );
        assert_eq!(
            error(
                &format!("{origin}/plain"),
                context(foreign, RequestMode::Cors),
                Default::default()
            )
            .await,
            "Network: Response blocked by native document policy"
        );
        assert_eq!(
            error(
                &format!("{origin}/loop"),
                context(&origin, RequestMode::NoCors),
                Default::default()
            )
            .await,
            "Script redirect limit exceeded"
        );
        let admitted = run(
            &format!("{origin}/js"),
            context(foreign, RequestMode::NoCors),
            Default::default(),
        )
        .await
        .unwrap();
        assert!(admitted.muted_errors);
        assert_eq!(admitted.response.body_bytes, b"globalThis.loaded=true;");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn data_urls_are_basic_responses_but_never_redirect_targets() {
        let origin = server().await;
        let foreign = "http://foreign.test/";
        let script = run(
            "data:text/javascript;charset=windows-1252,a=%E9#frag",
            context(foreign, RequestMode::NoCors),
            Default::default(),
        )
        .await
        .unwrap();
        assert!(!script.muted_errors);
        let response = script.response;
        assert_eq!(response.html, "a=\u{e9}");
        assert_eq!(response.body_bytes, b"a=\xe9");
        assert_eq!(
            response.final_url,
            "data:text/javascript;charset=windows-1252,a=%E9"
        );
        assert_eq!(
            (
                response.status,
                response.status_text.as_str(),
                response.redirected
            ),
            (200, "OK", false)
        );
        assert_eq!(
            response.content_type,
            "text/javascript;charset=windows-1252"
        );
        assert_eq!(
            response.headers,
            [(
                "content-type".to_string(),
                "text/javascript;charset=windows-1252".to_string()
            )]
        );
        let ascii = run(
            "data:,%C3%A9",
            context(foreign, RequestMode::SameOrigin),
            Default::default(),
        )
        .await
        .unwrap();
        // The default text/plain;charset=US-ASCII decodes as windows-1252, as in
        // Chrome 156 (`data:,r.push('%C3%A9')` pushes "\u{c3}\u{a9}").
        assert_eq!(ascii.response.html, "\u{c3}\u{a9}");
        assert_eq!(ascii.response.content_type, "text/plain;charset=US-ASCII");
        async fn integrity(value: &str) -> Result<ScriptResponse, String> {
            let mut jar = CookieJar::new();
            fetch(
                "data:,x",
                context("http://foreign.test/", RequestMode::NoCors),
                BTreeMap::new(),
                &ClientHints::default(),
                &mut jar,
                Default::default(),
                value,
            )
            .await
        }
        // sha256("x")
        let good = "sha256-LXEWQrcmsEQBYnyp+6wy9chTD7GQPMTbAiWHF5IaSIE=";
        assert_eq!(integrity(good).await.unwrap().response.body_bytes, b"x");
        let bad = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        assert_eq!(
            integrity(bad).await.err().unwrap(),
            "Script response failed integrity validation"
        );
        assert_eq!(
            error(
                "data:text/javascript;base64,%%%",
                context(foreign, RequestMode::NoCors),
                Default::default()
            )
            .await,
            "Script data: URL is not valid"
        );
        assert_eq!(
            error(
                &format!("{origin}/todata"),
                context(&origin, RequestMode::NoCors),
                Default::default()
            )
            .await,
            "Script URL is not an allowed HTTP URL"
        );
        let deny = |_: &str, _: bool| false;
        let csp = ScriptPolicies {
            request: Some(&deny),
            ..Default::default()
        };
        assert_eq!(
            error("data:,x", context(foreign, RequestMode::NoCors), csp).await,
            "Script blocked by Content Security Policy"
        );
    }

    #[test]
    fn javascript_mime_types_follow_the_html_essence_list() {
        for mime in [
            "text/javascript",
            "Application/X-JavaScript; charset=utf-8",
            " text/javascript1.5 ",
        ] {
            assert!(javascript_mime(mime), "{mime}");
        }
        for mime in [
            "application/json",
            "text/javascript1.6",
            "",
            "text/plain;text/javascript",
        ] {
            assert!(!javascript_mime(mime), "{mime}");
        }
    }
}
