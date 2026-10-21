//! CSP reports use native profile-owned jobs. Neither stalled IO nor queued bytes
//! retain an isolate, document, creation context, guest promise, or cookie Rc.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use turbo_surf_core::net::ReportClient as Client;
use turbo_surf_core::request_context::{same_origin, RequestContext};

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub url: String,
    pub modern: bool,
    pub credentials: bool,
}

pub(crate) fn endpoints(base: &str, headers: &[(String, String)]) -> BTreeMap<String, Endpoint> {
    let Ok(base_url) = url::Url::parse(base) else {
        return BTreeMap::new();
    };
    if base_url.scheme() != "https" {
        return BTreeMap::new();
    }
    let value = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("reporting-endpoints"))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut endpoints = BTreeMap::new();
    if let Ok(dictionary) = sfv::Parser::new(&value)
        .with_version(sfv::Version::Rfc8941)
        .parse::<sfv::Dictionary>()
    {
        for (name, entry) in dictionary {
            let sfv::ListEntry::Item(item) = entry else {
                continue;
            };
            let Some(value) = item.bare_item.as_string() else {
                continue;
            };
            if let Ok(url) = base_url.join(value.as_str()) {
                if url.scheme() == "https" {
                    endpoints.insert(
                        name.as_str().into(),
                        Endpoint {
                            url: url.into(),
                            modern: true,
                            credentials: true,
                        },
                    );
                }
            }
        }
    }
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("report-to"))
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(value) else {
            continue;
        };
        let values = value.as_array().cloned().unwrap_or_else(|| vec![value]);
        for value in values {
            if value.get("max_age").and_then(|v| v.as_u64()).unwrap_or(0) == 0 {
                continue;
            }
            let group = value
                .get("group")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let Some(url) = value
                .get("endpoints")
                .and_then(|v| v.as_array())
                .and_then(|list| list.first())
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
                .and_then(|value| url::Url::parse(value).ok())
            else {
                continue;
            };
            if url.scheme() == "https" {
                endpoints.entry(group.into()).or_insert(Endpoint {
                    url: url.into(),
                    modern: true,
                    credentials: false,
                });
            }
        }
    }
    endpoints
}

fn sample_json(units: &[u16]) -> Vec<u8> {
    let mut result = vec![b'"'];
    let mut index = 0;
    while index < units.len() {
        let mut value = u32::from(units[index]);
        index += 1;
        if (0xd800..=0xdbff).contains(&value)
            && index < units.len()
            && (0xdc00..=0xdfff).contains(&units[index])
        {
            value = 0x10000 + ((value - 0xd800) << 10) + (u32::from(units[index]) - 0xdc00);
            index += 1;
        }
        match value {
            34 | 92 => {
                result.push(b'\\');
                result.push(value as u8);
            }
            8 => result.extend(b"\\b"),
            12 => result.extend(b"\\f"),
            10 => result.extend(b"\\n"),
            13 => result.extend(b"\\r"),
            9 => result.extend(b"\\t"),
            0..=31 => result.extend(format!("\\u{value:04x}").bytes()),
            32..=127 => result.push(value as u8),
            128..=0x7ff => {
                result.extend([0xc0 | ((value >> 6) as u8), 0x80 | ((value & 63) as u8)])
            }
            0x800..=0xffff => result.extend([
                0xe0 | ((value >> 12) as u8),
                0x80 | (((value >> 6) & 63) as u8),
                0x80 | ((value & 63) as u8),
            ]),
            _ => result.extend([
                0xf0 | ((value >> 18) as u8),
                0x80 | (((value >> 12) & 63) as u8),
                0x80 | (((value >> 6) & 63) as u8),
                0x80 | ((value & 63) as u8),
            ]),
        }
    }
    result.push(b'"');
    result
}

pub(crate) fn body(metadata: &serde_json::Value, sample: &[u16], modern: bool) -> Vec<u8> {
    let mapping = if modern {
        &[
            ("documentURI", "documentURL"),
            ("referrer", "referrer"),
            ("effectiveDirective", "effectiveDirective"),
            ("originalPolicy", "originalPolicy"),
            ("disposition", "disposition"),
            ("blockedURI", "blockedURL"),
            ("lineNumber", "lineNumber"),
            ("columnNumber", "columnNumber"),
            ("sourceFile", "sourceFile"),
            ("statusCode", "statusCode"),
        ][..]
    } else {
        &[
            ("documentURI", "document-uri"),
            ("referrer", "referrer"),
            ("violatedDirective", "violated-directive"),
            ("effectiveDirective", "effective-directive"),
            ("originalPolicy", "original-policy"),
            ("disposition", "disposition"),
            ("blockedURI", "blocked-uri"),
            ("lineNumber", "line-number"),
            ("columnNumber", "column-number"),
            ("sourceFile", "source-file"),
            ("statusCode", "status-code"),
        ][..]
    };
    let mut payload = Vec::new();
    payload.push(b'{');
    for (source, key) in mapping {
        payload.extend(serde_json::to_vec(key).unwrap());
        payload.push(b':');
        payload.extend(serde_json::to_vec(&metadata[*source]).unwrap());
        payload.push(b',');
    }
    payload.extend(if modern {
        b"\"sample\":".as_slice()
    } else {
        b"\"script-sample\":".as_slice()
    });
    payload.extend(sample_json(sample));
    payload.push(b'}');
    if modern {
        let prefix=serde_json::json!({"age":0,"type":"csp-violation","url":metadata["documentURI"],"user_agent":turbo_surf_core::fingerprint::default_profile().user_agent}).to_string();
        let mut result = vec![b'['];
        result.extend(prefix[..prefix.len() - 1].bytes());
        result.extend(b",\"body\":");
        result.extend(String::from_utf8_lossy(&payload).as_bytes());
        result.extend(b"}]");
        result
    } else {
        let mut result = b"{\"csp-report\":".to_vec();
        result.extend(payload);
        result.push(b'}');
        result
    }
}

#[derive(Default)]
struct Jobs {
    count: usize,
    bytes: usize,
    cookies: Vec<(u64, String, Vec<String>)>,
    cookie_bytes: usize,
}
struct Owner {
    cancel: watch::Sender<bool>,
    jobs: Arc<Mutex<Jobs>>,
    client: Client,
    response_effects: Mutex<Option<Arc<turbo_surf_core::net::ResponseSideEffects<'static>>>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}
#[derive(Clone)]
pub(crate) struct Reporter(Arc<Owner>);
impl Default for Reporter {
    fn default() -> Self {
        let (cancel, _) = watch::channel(false);
        Self(Arc::new(Owner {
            cancel,
            jobs: Arc::default(),
            client: turbo_surf_core::net::build_report_client(),
            response_effects: Mutex::new(None),
        }))
    }
}
struct Permit {
    jobs: Arc<Mutex<Jobs>>,
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.count -= 1;
        jobs.bytes -= self.bytes;
    }
}
pub(crate) struct ReportJob {
    pub endpoint: Endpoint,
    pub context: RequestContext,
    pub jar: turbo_surf_core::cookies::CookieJar,
    pub target: u64,
    pub client_hints: turbo_surf_core::request_context::ClientHints,
    pub body: Vec<u8>,
    pub response_effects: Option<Arc<turbo_surf_core::net::ResponseSideEffects<'static>>>,
}
impl Reporter {
    pub(crate) fn with_client(client: Client) -> Self {
        let (cancel, _) = watch::channel(false);
        Self(Arc::new(Owner {
            cancel,
            jobs: Arc::default(),
            client,
            response_effects: Mutex::new(None),
        }))
    }
    pub(crate) fn set_response_effects(
        &self,
        effects: Arc<turbo_surf_core::net::ResponseSideEffects<'static>>,
    ) {
        *self.0.response_effects.lock().unwrap() = Some(effects);
    }
    pub(crate) fn diagnostics(&self) -> serde_json::Value {
        let jobs = self.0.jobs.lock().unwrap();
        serde_json::json!({"jobs":jobs.count,"bytes":jobs.bytes,"cookieEffects":jobs.cookies.len()})
    }
    pub(crate) fn drain_cookies(&self) -> Vec<(u64, String, Vec<String>)> {
        let mut jobs = self.0.jobs.lock().unwrap();
        jobs.cookie_bytes = 0;
        std::mem::take(&mut jobs.cookies)
    }
    pub(crate) fn send(&self, job: ReportJob) {
        let ReportJob {
            endpoint,
            mut context,
            mut jar,
            target,
            client_hints,
            body,
            response_effects: override_effects,
        } = job;
        let bytes = body.len().saturating_mul(2) + endpoint.url.len() + jar.storage_state().len();
        {
            let mut jobs = self.0.jobs.lock().unwrap();
            if jobs.count >= 4096 || jobs.bytes.saturating_add(bytes) > 64 * 1024 * 1024 {
                return;
            }
            jobs.count += 1;
            jobs.bytes += bytes;
        }
        let permit = Permit {
            jobs: self.0.jobs.clone(),
            bytes,
        };
        let client = self.0.client.clone();
        // Capture only native backing: retaining a profile here would keep the
        // cancellation sender alive throughout stalled IO.
        let profile_effects = if target == 0 {
            self.0.response_effects.lock().unwrap().clone()
        } else {
            None
        };
        let response_effects: Option<Arc<turbo_surf_core::net::ResponseSideEffects<'static>>> =
            match (override_effects, profile_effects) {
                (Some(partition), Some(profile)) => Some(Arc::new(move |context, url, headers| {
                    partition(context, url, headers);
                    profile(context, url, headers);
                })),
                (partition, profile) => partition.or(profile),
            };
        let mut canceled = self.0.cancel.subscribe();
        context.credentials = if endpoint.credentials {
            turbo_surf_core::request_context::Credentials::SameOrigin
        } else {
            turbo_surf_core::request_context::Credentials::Omit
        };
        tokio::spawn(async move {
            let request = async {
                let mut target_url = endpoint.url;
                let mut method = "POST";
                let mut body = body;
                for hop in 0..=20 {
                    let credentials = context.sends_credentials(&target_url);
                    let origin = if context.opaque_origin || context.origin_tainted {
                        "null".into()
                    } else {
                        context
                            .source
                            .as_deref()
                            .and_then(|source| url::Url::parse(source).ok())
                            .map(|url| url.origin().ascii_serialization())
                            .unwrap_or_else(|| "null".into())
                    };
                    let cross_origin = !context
                        .source
                        .as_deref()
                        .is_some_and(|source| same_origin(source, &target_url));
                    let mut headers = if endpoint.modern {
                        BTreeMap::new()
                    } else {
                        context.headers(&target_url, method)
                    };
                    let profile = turbo_surf_core::fingerprint::default_profile();
                    headers.insert("accept-language".into(), profile.accept_language.into());
                    headers.insert("accept-encoding".into(), "gzip, deflate, br, zstd".into());
                    if !endpoint.modern {
                        headers.remove("priority");
                        headers.extend(client_hints.headers(&target_url, &context, &profile));
                    }
                    headers.insert(
                        "user-agent".into(),
                        turbo_surf_core::fingerprint::default_profile().user_agent,
                    );
                    if method == "POST" {
                        headers.insert(
                            "content-type".into(),
                            if endpoint.modern {
                                "application/reports+json"
                            } else {
                                "application/csp-report"
                            }
                            .into(),
                        );
                    }
                    if method == "POST" || cross_origin {
                        headers.insert("origin".into(), origin.clone());
                    }
                    if credentials {
                        let cookie = jar.cookie_header_for_request(
                            &target_url,
                            crate::runtime::cookie_now(),
                            &context,
                            method,
                        );
                        if !cookie.is_empty() {
                            headers.insert("cookie".into(), cookie);
                        }
                    }
                    if endpoint.modern && cross_origin && method == "POST" {
                        let mut preflight = BTreeMap::from([
                            ("origin".into(), origin.clone()),
                            ("access-control-request-method".into(), "POST".into()),
                            (
                                "access-control-request-headers".into(),
                                "content-type".into(),
                            ),
                        ]);
                        preflight.insert("user-agent".into(), profile.user_agent.clone());
                        preflight.insert("accept-language".into(), profile.accept_language.into());
                        preflight
                            .insert("accept-encoding".into(), "gzip, deflate, br, zstd".into());
                        let response = turbo_surf_core::net::send_report_request(
                            &client,
                            "OPTIONS",
                            &target_url,
                            &preflight,
                            Vec::new(),
                            true,
                        )
                        .await
                        .ok()?;
                        if !(200..300).contains(&response.status)
                            || !crate::fetch_cors::preflight_allowed(
                                &context,
                                "POST",
                                &["content-type".into()],
                                &response.headers,
                            )
                        {
                            return None;
                        }
                    }
                    let response = turbo_surf_core::net::send_report_request(
                        &client,
                        method,
                        &target_url,
                        &headers,
                        body.clone(),
                        endpoint.modern,
                    )
                    .await
                    .ok()?;
                    if credentials {
                        let clear_hints = response
                            .headers
                            .iter()
                            .filter(|(name, _)| name.eq_ignore_ascii_case("clear-site-data"))
                            .flat_map(|(_, value)| value.split(','))
                            .any(|token| {
                                matches!(token.trim(), "\"clientHints\"" | "\"cache\"" | "\"*\"")
                            });
                        if clear_hints {
                            client_hints.observe(&target_url, None, true);
                        }
                        if let Some(effects) = &response_effects {
                            effects(&context, &target_url, &response.headers);
                        }
                        let lines = response
                            .headers
                            .iter()
                            .filter(|(key, _)| key.eq_ignore_ascii_case("set-cookie"))
                            .map(|(_, value)| value.clone())
                            .collect::<Vec<_>>();
                        if !lines.is_empty() {
                            jar.set_from_response(
                                &target_url,
                                &lines,
                                crate::runtime::cookie_now(),
                            );
                            let mut jobs = permit.jobs.lock().unwrap();
                            let bytes =
                                target_url.len() + lines.iter().map(String::len).sum::<usize>();
                            if jobs.cookies.len() < 4096
                                && jobs.cookie_bytes.saturating_add(bytes) <= 64 * 1024 * 1024
                            {
                                jobs.cookie_bytes += bytes;
                                jobs.cookies.push((target, target_url.clone(), lines));
                            }
                        }
                    }
                    if endpoint.modern
                        && cross_origin
                        && !crate::fetch_cors::allowed(&context, &response.headers)
                    {
                        return None;
                    }
                    if !endpoint.modern || !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                        return Some(());
                    }
                    if hop == 20 {
                        return None;
                    }
                    let location = response
                        .headers
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("location"))?
                        .1
                        .as_str();
                    let next = url::Url::parse(&target_url).ok()?.join(location).ok()?;
                    if !matches!(next.scheme(), "http" | "https")
                        || !next.username().is_empty()
                        || next.password().is_some()
                    {
                        return None;
                    }
                    context.follow_redirect(&target_url, next.as_str());
                    target_url = next.into();
                    if response.status == 303
                        || matches!(response.status, 301 | 302) && method == "POST"
                    {
                        method = "GET";
                        body.clear();
                    }
                }
                None
            };
            tokio::select! {_ = request=>{},_ = canceled.changed()=>{}}
            drop(permit);
        });
    }
}
