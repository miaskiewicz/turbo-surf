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
            jar,
            target,
            client_hints,
            body,
            response_effects: override_effects,
        } = job;
        let Some(permit) = self
            .admit(body.len().saturating_mul(2) + endpoint.url.len() + jar.storage_state().len())
        else {
            return;
        };
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
        let delivery = Delivery {
            endpoint,
            context,
            jar,
            target,
            client: self
                .0
                .client
                .with_network_conditions(client_hints.network_conditions()),
            client_hints,
            response_effects,
            permit,
        };
        tokio::spawn(async move {
            tokio::select! {_ = delivery.run(body)=>{},_ = canceled.changed()=>{}}
        });
    }
    /// Reserves queue capacity (4096 jobs, 64MiB) for a job of `bytes`.
    fn admit(&self, bytes: usize) -> Option<Permit> {
        let mut jobs = self.0.jobs.lock().unwrap();
        if jobs.count >= 4096 || jobs.bytes.saturating_add(bytes) > 64 * 1024 * 1024 {
            return None;
        }
        jobs.count += 1;
        jobs.bytes += bytes;
        Some(Permit {
            jobs: self.0.jobs.clone(),
            bytes,
        })
    }
}

/// One report upload's native state, owned by its task; dropping it releases the
/// queue permit.
struct Delivery {
    endpoint: Endpoint,
    context: RequestContext,
    jar: turbo_surf_core::cookies::CookieJar,
    target: u64,
    client: Client,
    client_hints: turbo_surf_core::request_context::ClientHints,
    response_effects: Option<Arc<turbo_surf_core::net::ResponseSideEffects<'static>>>,
    permit: Permit,
}

/// One hop's request: where it goes and how.
struct Hop<'a> {
    url: &'a str,
    method: &'a str,
    origin: &'a str,
    cross_origin: bool,
    credentials: bool,
}

impl Delivery {
    async fn run(mut self, mut body: Vec<u8>) -> Option<()> {
        let mut target_url = self.endpoint.url.clone();
        let mut method = "POST";
        for hop in 0.. {
            let origin = self.origin();
            let request = Hop {
                url: &target_url,
                method,
                origin: &origin,
                cross_origin: !self
                    .context
                    .source
                    .as_deref()
                    .is_some_and(|source| same_origin(source, &target_url)),
                credentials: self.context.sends_credentials(&target_url),
            };
            let response = self.exchange(&request, body.clone()).await?;
            if !self.endpoint.modern || !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                return Some(());
            }
            if hop == 20 {
                return None;
            }
            let next = redirect_target(&target_url, &response.headers)?;
            self.context.follow_redirect(&target_url, next.as_str());
            target_url = next.into();
            if response.status == 303 || matches!(response.status, 301 | 302) && method == "POST" {
                method = "GET";
                body.clear();
            }
        }
        // Every hop returns; hop 20 stops redirects before the range ends.
        unreachable!("report redirect loop is bounded by hop 20")
    }
    /// One hop: its preflight when needed, the request, and the response's credentialed
    /// side effects. `None` stops delivery (network failure or refused CORS).
    async fn exchange(
        &mut self,
        hop: &Hop<'_>,
        body: Vec<u8>,
    ) -> Option<turbo_surf_core::net::ReportResponse> {
        let headers = self.headers(hop);
        if self.endpoint.modern && hop.cross_origin && hop.method == "POST" {
            self.preflight(hop).await?;
        }
        let response = turbo_surf_core::net::send_report_request(
            &self.client,
            hop.method,
            hop.url,
            &headers,
            body,
            self.endpoint.modern,
        )
        .await
        .ok()?;
        if hop.credentials {
            self.observe_credentialed(hop.url, &response.headers);
        }
        if self.endpoint.modern
            && hop.cross_origin
            && !crate::fetch_cors::allowed(&self.context, &response.headers)
        {
            return None;
        }
        Some(response)
    }
    fn origin(&self) -> String {
        if self.context.opaque_origin || self.context.origin_tainted {
            return "null".into();
        }
        self.context
            .source
            .as_deref()
            .and_then(|source| url::Url::parse(source).ok())
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|| "null".into())
    }
    fn headers(&self, hop: &Hop) -> BTreeMap<String, String> {
        let modern = self.endpoint.modern;
        let mut headers = if modern {
            BTreeMap::new()
        } else {
            self.context.headers(hop.url, hop.method)
        };
        let profile = turbo_surf_core::fingerprint::default_profile();
        headers.insert("accept-language".into(), profile.accept_language.into());
        headers.insert("accept-encoding".into(), "gzip, deflate, br, zstd".into());
        if !modern {
            headers.remove("priority");
            headers.extend(self.client_hints.headers(hop.url, &self.context, &profile));
        }
        headers.insert(
            "user-agent".into(),
            turbo_surf_core::fingerprint::default_profile().user_agent,
        );
        if hop.method == "POST" {
            let content_type = if modern {
                "application/reports+json"
            } else {
                "application/csp-report"
            };
            headers.insert("content-type".into(), content_type.into());
        }
        if hop.method == "POST" || hop.cross_origin {
            headers.insert("origin".into(), hop.origin.to_owned());
        }
        if hop.credentials {
            let cookie = self.jar.cookie_header_for_request(
                hop.url,
                crate::runtime::cookie_now(),
                &self.context,
                hop.method,
            );
            if !cookie.is_empty() {
                headers.insert("cookie".into(), cookie);
            }
        }
        headers
    }
    /// The CORS preflight of a cross-origin modern report; `None` refuses the upload.
    async fn preflight(&self, hop: &Hop<'_>) -> Option<()> {
        let profile = turbo_surf_core::fingerprint::default_profile();
        let mut preflight = BTreeMap::from([
            ("origin".into(), hop.origin.to_owned()),
            ("access-control-request-method".into(), "POST".into()),
            (
                "access-control-request-headers".into(),
                "content-type".into(),
            ),
        ]);
        preflight.insert("user-agent".into(), profile.user_agent.clone());
        preflight.insert("accept-language".into(), profile.accept_language.into());
        preflight.insert("accept-encoding".into(), "gzip, deflate, br, zstd".into());
        let response = turbo_surf_core::net::send_report_request(
            &self.client,
            "OPTIONS",
            hop.url,
            &preflight,
            Vec::new(),
            true,
        )
        .await
        .ok()?;
        ((200..300).contains(&response.status)
            && crate::fetch_cors::preflight_allowed(
                &self.context,
                "POST",
                &["content-type".into()],
                &response.headers,
            ))
        .then_some(())
    }
    /// A credentialed response's Clear-Site-Data hints, observer effects and cookies.
    fn observe_credentialed(&mut self, target_url: &str, headers: &[(String, String)]) {
        let clear_hints = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("clear-site-data"))
            .flat_map(|(_, value)| value.split(','))
            .any(|token| matches!(token.trim(), "\"clientHints\"" | "\"cache\"" | "\"*\""));
        if clear_hints {
            self.client_hints.observe(target_url, None, true);
        }
        if let Some(effects) = &self.response_effects {
            effects(&self.context, target_url, headers);
        }
        let lines = headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        if !lines.is_empty() {
            self.store_cookies(target_url, lines);
        }
    }
    /// Applies Set-Cookie lines to the job's jar and queues them for the profile,
    /// bounded like the jobs (4096 entries, 64MiB).
    fn store_cookies(&mut self, target_url: &str, lines: Vec<String>) {
        self.jar
            .set_from_response(target_url, &lines, crate::runtime::cookie_now());
        let mut jobs = self.permit.jobs.lock().unwrap();
        let bytes = target_url.len() + lines.iter().map(String::len).sum::<usize>();
        if jobs.cookies.len() < 4096 && jobs.cookie_bytes.saturating_add(bytes) <= 64 * 1024 * 1024
        {
            jobs.cookie_bytes += bytes;
            jobs.cookies
                .push((self.target, target_url.to_owned(), lines));
        }
    }
}

/// A modern report's redirect hop: http(s) only, never with credentials in the URL.
fn redirect_target(target_url: &str, headers: &[(String, String)]) -> Option<url::Url> {
    let location = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("location"))?
        .1
        .as_str();
    let next = url::Url::parse(target_url).ok()?.join(location).ok()?;
    (matches!(next.scheme(), "http" | "https")
        && next.username().is_empty()
        && next.password().is_none())
    .then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn summary(endpoints: &BTreeMap<String, Endpoint>) -> Vec<(String, String, bool)> {
        endpoints
            .iter()
            .map(|(name, endpoint)| {
                assert!(endpoint.modern);
                (name.clone(), endpoint.url.clone(), endpoint.credentials)
            })
            .collect()
    }

    #[test]
    fn endpoints_require_a_secure_document_and_secure_endpoints() {
        let both = headers(&[
            ("Reporting-Endpoints", "a=\"/r\""),
            (
                "Report-To",
                r#"{"group":"g","max_age":1,"endpoints":[{"url":"https://r.test/g"}]}"#,
            ),
        ]);
        assert!(endpoints("not a url", &both).is_empty());
        assert!(endpoints("http://site.test/", &both).is_empty());
        assert_eq!(
            summary(&endpoints("https://site.test/page", &both)),
            [
                ("a".into(), "https://site.test/r".into(), true),
                ("g".into(), "https://r.test/g".into(), false)
            ]
        );
    }

    #[test]
    fn reporting_endpoints_skip_inner_lists_tokens_and_insecure_urls() {
        let list = headers(&[(
            "reporting-endpoints",
            "list=(\"/a\" \"/b\"), token=bare, plain=\"http://x.test/r\", ok=\"https://ok.test/r\"",
        )]);
        assert_eq!(
            summary(&endpoints("https://site.test/", &list)),
            [("ok".into(), "https://ok.test/r".into(), true)]
        );
    }

    #[test]
    fn report_to_skips_invalid_expired_endpointless_and_insecure_groups() {
        let groups = headers(&[
            ("report-to", "{not json"),
            (
                "report-to",
                r#"{"group":"expired","max_age":0,"endpoints":[{"url":"https://e.test/"}]}"#,
            ),
            (
                "report-to",
                r#"{"group":"none","max_age":5,"endpoints":[]}"#,
            ),
            (
                "report-to",
                r#"{"group":"bad","max_age":5,"endpoints":[{"url":"::"}]}"#,
            ),
            (
                "report-to",
                r#"{"group":"plain","max_age":5,"endpoints":[{"url":"http://p.test/"}]}"#,
            ),
            (
                "report-to",
                r#"[{"max_age":5,"endpoints":[{"url":"https://d.test/1"}]},{"max_age":5,"endpoints":[{"url":"https://d.test/2"}]}]"#,
            ),
        ]);
        assert_eq!(
            summary(&endpoints("https://site.test/", &groups)),
            [("default".into(), "https://d.test/1".into(), false)]
        );
    }

    #[test]
    fn report_samples_serialize_escapes_and_every_utf8_width() {
        let text = "\"\\\u{8}\u{c}\n\r\t\u{1}aé€😀";
        let units = text.encode_utf16().collect::<Vec<_>>();
        let encoded = sample_json(&units);
        assert_eq!(
            encoded,
            "\"\\\"\\\\\\b\\f\\n\\r\\t\\u0001aé€😀\"".as_bytes()
        );
        assert_eq!(serde_json::from_slice::<String>(&encoded).unwrap(), text);
        // A high surrogate without its pair is encoded alone, not merged with the next unit.
        assert_eq!(sample_json(&[0xd83d, 0x41]), b"\"\xed\xa0\xbdA\"");
        assert_eq!(sample_json(&[0xd83d]), b"\"\xed\xa0\xbd\"");
    }
}
