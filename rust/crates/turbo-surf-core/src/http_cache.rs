//! Session-owned private HTTP cache. Transport streams publish only complete bodies.
use crate::http_cache_ranges::{ByteRange, SparseBody};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CacheMode {
    #[default]
    Default,
    NoStore,
    Reload,
    NoCache,
    ForceCache,
    OnlyIfCached,
}
impl CacheMode {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "default" => Self::Default,
            "no-store" => Self::NoStore,
            "reload" => Self::Reload,
            "no-cache" => Self::NoCache,
            "force-cache" => Self::ForceCache,
            "only-if-cached" => Self::OnlyIfCached,
            _ => return None,
        })
    }
}

#[derive(Clone, Default)]
pub struct HttpCache(Arc<Mutex<CacheState>>);
#[derive(Default)]
struct CacheState {
    entries: Vec<Entry>,
    generation: u64,
}
#[derive(Clone)]
pub(crate) struct Entry {
    key: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub version: http::Version,
    vary: Vec<(String, Option<String>)>,
    stored: SystemTime,
    generation: u64,
    sparse: Option<SparseBody>,
    received_range: Option<ByteRange>,
}
#[derive(Clone)]
pub(crate) struct PendingStore {
    pub cache: HttpCache,
    pub entry: Entry,
    pub skip_prefix: usize,
}
#[derive(Clone)]
pub(crate) struct Cached(pub String);
#[derive(Clone)]
pub(crate) struct TracePrefix(pub usize);
fn values(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
fn directives(headers: &[(String, String)]) -> BTreeMap<String, String> {
    values(headers, "cache-control")
        .split(',')
        .map(|value| {
            let (key, value) = value.trim().split_once('=').unwrap_or((value.trim(), ""));
            (key.to_ascii_lowercase(), value.trim_matches('"').to_owned())
        })
        .collect()
}
fn date(headers: &[(String, String)], name: &str) -> Option<SystemTime> {
    httpdate::parse_http_date(&values(headers, name)).ok()
}
impl Entry {
    pub fn fresh(&self) -> bool {
        let control = directives(&self.headers);
        if control.contains_key("no-cache") {
            return false;
        }
        let now = SystemTime::now();
        let response_date = date(&self.headers, "date").unwrap_or(self.stored);
        let age = values(&self.headers, "age").parse::<u64>().unwrap_or(0);
        let initial_age = self
            .stored
            .duration_since(response_date)
            .unwrap_or_default()
            .max(Duration::from_secs(age));
        let current_age = initial_age + now.duration_since(self.stored).unwrap_or_default();
        let lifetime = control
            .get("max-age")
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .or_else(|| {
                date(&self.headers, "expires")
                    .map(|expires| expires.duration_since(response_date).unwrap_or_default())
            })
            .or_else(|| {
                date(&self.headers, "last-modified")
                    .and_then(|modified| response_date.duration_since(modified).ok())
                    .map(|age| age / 10)
            })
            .unwrap_or_default();
        current_age < lifetime
    }
    pub fn validators(&self, headers: &mut BTreeMap<String, String>) {
        for (response, request) in [
            ("etag", "if-none-match"),
            ("last-modified", "if-modified-since"),
        ] {
            let value = values(&self.headers, response);
            if !value.is_empty() {
                headers.entry(request.into()).or_insert(value);
            }
        }
    }
    pub fn merge(&mut self, headers: &[(String, String)], request: &BTreeMap<String, String>) {
        let update = headers
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.to_ascii_lowercase().as_str(),
                    "connection" | "transfer-encoding" | "content-length" | "set-cookie"
                )
            })
            .collect::<Vec<_>>();
        self.headers
            .retain(|(old, _)| !update.iter().any(|(key, _)| old.eq_ignore_ascii_case(key)));
        self.headers.extend(update.into_iter().cloned());
        self.vary = values(&self.headers, "vary")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                let key = name.to_ascii_lowercase();
                let value = request.get(&key).cloned();
                (key, value)
            })
            .collect();
        self.stored = SystemTime::now();
    }
    pub fn sparse(&self) -> bool {
        self.sparse.is_some()
    }
    pub fn strong_validator(&self) -> Option<String> {
        let etag = values(&self.headers, "etag");
        (etag.starts_with('"') && etag.ends_with('"')).then_some(etag)
    }
    pub fn selected_range(&self, header: Option<&str>) -> Option<ByteRange> {
        let total = self
            .sparse
            .as_ref()
            .map_or(self.body.len(), |body| body.total);
        match header {
            Some(value) => ByteRange::request(value, total),
            None => Some(ByteRange {
                start: 0,
                end: total,
            }),
        }
    }
    pub fn prefix(&self, range: ByteRange) -> Vec<u8> {
        self.sparse.as_ref().map_or_else(
            || {
                self.body
                    .get(range.start..range.end)
                    .unwrap_or_default()
                    .to_vec()
            },
            |body| body.prefix(range),
        )
    }
    pub fn range_response(&self, header: Option<&str>) -> Option<Self> {
        if header.is_none() && self.sparse.is_none() {
            return Some(self.clone());
        }
        if !matches!(self.status, 200 | 206) {
            return None;
        }
        let range = self.selected_range(header)?;
        let body = if let Some(sparse) = &self.sparse {
            sparse.slice(range)?
        } else {
            self.body.get(range.start..range.end)?.to_vec()
        };
        let mut result = self.response_metadata(range, header.is_some());
        result.body = body;
        Some(result)
    }
    pub fn response_metadata(&self, range: ByteRange, is_range: bool) -> Self {
        let mut result = self.clone();
        result.status = if is_range { 206 } else { 200 };
        result.status_text = if self.version == http::Version::HTTP_2 {
            String::new()
        } else {
            if is_range { "Partial Content" } else { "OK" }.into()
        };
        result.headers.retain(|(key, _)| {
            !matches!(
                key.to_ascii_lowercase().as_str(),
                "content-range" | "content-length"
            )
        });
        result.headers.push((
            "content-length".into(),
            (range.end - range.start).to_string(),
        ));
        if is_range {
            result.headers.push((
                "content-range".into(),
                range.header(
                    self.sparse
                        .as_ref()
                        .map_or(self.body.len(), |body| body.total),
                ),
            ));
        }
        result
    }
    fn size(&self) -> usize {
        self.body.len() + self.sparse.as_ref().map_or(0, SparseBody::size)
    }
    pub fn response(&self) -> crate::http_backend::Response {
        self.response_body(crate::http_backend::Body::from(self.body.clone()))
    }
    pub fn response_body(&self, body: crate::http_backend::Body) -> crate::http_backend::Response {
        use crate::http_backend::ResponseBuilderExt;
        let builder = http::Response::builder()
            .status(self.status)
            .version(self.version);
        #[cfg(feature = "impersonate")]
        let mut builder = builder.uri(self.url.parse().expect("cached request URI"));
        #[cfg(not(feature = "impersonate"))]
        let mut builder = builder.url(url::Url::parse(&self.url).expect("cached request URL"));
        for (key, value) in &self.headers {
            builder = builder.header(key, value);
        }
        let mut response = builder.body(body).expect("valid cached response");
        response
            .extensions_mut()
            .insert(Cached(self.status_text.clone()));
        response.into()
    }
}
impl HttpCache {
    pub(crate) fn lookup(
        &self,
        partition: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
    ) -> Option<(Entry, bool)> {
        let key = format!("{partition}\n{}", normalized_url(url));
        let state = self.0.lock().unwrap();
        state
            .entries
            .iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| {
                let mut entry = entry.clone();
                entry.generation = state.generation;
                let matches = entry
                    .vary
                    .iter()
                    .all(|(key, value)| key != "*" && headers.get(key) == value.as_ref());
                (entry, matches)
            })
    }
    pub(crate) fn generation(&self) -> u64 {
        self.0.lock().unwrap().generation
    }
    /// Observe raw network headers, including native report responses. Cached
    /// metadata must never be passed here: replay does not cause new side effects.
    pub fn observe_response(
        &self,
        context: &crate::request_context::RequestContext,
        url: &str,
        headers: &[(String, String)],
    ) {
        if !context.sends_credentials(url) {
            return;
        }
        let value = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("clear-site-data"))
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            .join(",");
        self.clear_response(url, &value);
    }
    pub(crate) fn clear_response(&self, url: &str, value: &str) {
        if !crate::request_context::trustworthy(url)
            || !value
                .split(',')
                .any(|part| matches!(part.trim(), "\"cache\"" | "\"*\""))
        {
            return;
        }
        let Some(origin) = crate::request_context::origin(url) else {
            return;
        };
        let mut state = self.0.lock().unwrap();
        state
            .entries
            .retain(|entry| crate::request_context::origin(&entry.url).as_deref() != Some(&origin));
        state.generation = state.generation.wrapping_add(1);
    }
    pub(crate) fn pending(
        &self,
        partition: &str,
        url: &str,
        request: &BTreeMap<String, String>,
        response: &crate::http_backend::Response,
        status_text: String,
        generation: u64,
    ) -> Option<PendingStore> {
        let headers: Vec<_> = response
            .headers()
            .iter()
            .filter_map(|(key, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (key.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        let control = directives(&headers);
        let vary = values(&headers, "vary");
        if control.contains_key("no-store") {
            self.invalidate(partition, url);
            return None;
        }
        let status = response.status().as_u16();
        let explicit_freshness = control
            .get("max-age")
            .is_some_and(|value| value.parse::<u64>().is_ok())
            || date(&headers, "expires").is_some();
        let cacheable = matches!(
            status,
            200 | 203 | 204 | 300 | 301 | 308 | 404 | 405 | 410 | 414 | 501
        ) || (200..600).contains(&status) && explicit_freshness;
        let received_range = (status == 206)
            .then(|| ByteRange::content(&values(&headers, "content-range")))
            .flatten();
        let sparse = received_range.map(|(_, total)| SparseBody::new(total));
        if status == 206 && (sparse.is_none() || !values(&headers, "etag").starts_with('"')) {
            return None;
        }
        if !cacheable || status == 304 {
            return None;
        }
        Some(PendingStore {
            cache: self.clone(),
            entry: Entry {
                key: format!("{partition}\n{}", normalized_url(url)),
                url: normalized_url(url),
                status: response.status().as_u16(),
                status_text,
                headers,
                body: Vec::new(),
                version: response.version(),
                vary: vary
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(|name| {
                        let name = name.to_ascii_lowercase();
                        let value = request.get(&name).cloned();
                        (name, value)
                    })
                    .collect(),
                stored: SystemTime::now(),
                generation,
                sparse,
                received_range: received_range.map(|(range, _)| range),
            },
            skip_prefix: 0,
        })
    }
    pub(crate) fn store(&self, mut entry: Entry) {
        const LIMIT: usize = 32 * 1024 * 1024;
        let incoming_range = entry.received_range.take();
        if let Some(range) = incoming_range {
            let bytes = std::mem::take(&mut entry.body);
            if !entry
                .sparse
                .as_mut()
                .is_some_and(|sparse| sparse.insert(range, bytes))
            {
                return;
            }
        }
        if entry.size() > LIMIT {
            return;
        }
        let mut state = self.0.lock().unwrap();
        if entry.generation != state.generation {
            return;
        }
        let entries = &mut state.entries;
        if directives(&entry.headers).contains_key("no-store") {
            entries.retain(|old| old.key != entry.key);
            return;
        }
        if let Some(old) = entries.iter().find(|old| {
            incoming_range.is_some()
                && old.key == entry.key
                && old.vary == entry.vary
                && old.strong_validator().is_some()
                && old.strong_validator() == entry.strong_validator()
        }) {
            if let (Some(previous), Some(received)) = (&old.sparse, &entry.sparse) {
                if previous.total == received.total {
                    let mut sparse = previous.clone();
                    sparse.append(received.clone());
                    entry.sparse = Some(sparse);
                }
            } else if old.status == 200
                && entry
                    .sparse
                    .as_ref()
                    .is_some_and(|sparse| sparse.total == old.body.len())
            {
                let mut complete = old.clone();
                complete.merge(&entry.headers, &BTreeMap::new());
                complete
                    .headers
                    .retain(|(key, _)| !key.eq_ignore_ascii_case("content-range"));
                complete.vary = entry.vary;
                complete.generation = entry.generation;
                entry = complete;
            }
        }
        if entry.size() > LIMIT {
            return;
        }
        entries.retain(|old| old.key != entry.key);
        while entries.len() >= 512
            || entries.iter().map(|entry| entry.size()).sum::<usize>() + entry.size() > LIMIT
        {
            entries.remove(0);
        }
        entries.push(entry);
    }
    pub(crate) fn invalidate(&self, partition: &str, url: &str) {
        let key = format!("{partition}\n{}", normalized_url(url));
        self.0
            .lock()
            .unwrap()
            .entries
            .retain(|entry| entry.key != key);
    }
}
fn normalized_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return value.into();
    };
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(headers: &[(&str, String)]) -> Entry {
        Entry {
            key: "partition\nhttps://cache.test/".into(),
            url: "https://cache.test/".into(),
            status: 200,
            status_text: "OK".into(),
            headers: headers
                .iter()
                .map(|(key, value)| ((*key).into(), value.clone()))
                .collect(),
            body: vec![0, 255],
            version: http::Version::HTTP_11,
            vary: Vec::new(),
            stored: SystemTime::now(),
            generation: 0,
            sparse: None,
            received_range: None,
        }
    }
    #[test]
    fn clear_site_data_is_trusted_origin_scoped_and_prevents_old_body_stores() {
        let cache = HttpCache::default();
        let old = entry(&[("cache-control", "max-age=3600".into())]);
        cache.store(old.clone());
        let mut other = old.clone();
        other.key = "partition\nhttps://other.test/".into();
        other.url = "https://other.test/".into();
        cache.store(other);
        cache.clear_response("https://cache.test/clear", "cache");
        assert!(cache
            .lookup("partition", "https://cache.test/", &BTreeMap::new())
            .is_some());
        cache.clear_response("http://untrusted.test/clear", "\"cache\"");
        assert!(cache
            .lookup("partition", "https://cache.test/", &BTreeMap::new())
            .is_some());
        cache.clear_response("https://cache.test/clear", "\"cache\"");
        cache.store(old);
        assert!(cache
            .lookup("partition", "https://cache.test/", &BTreeMap::new())
            .is_none());
        assert!(cache
            .lookup("partition", "https://other.test/", &BTreeMap::new())
            .is_some());
    }
    #[test]
    fn raw_response_observer_requires_credentials_and_invalidates_inflight_stores() {
        use crate::request_context::{Credentials, Destination, RequestMode};
        let cache = HttpCache::default();
        let old = entry(&[("cache-control", "max-age=3600".into())]);
        cache.store(old.clone());
        let mut context = crate::request_context::RequestContext::resource(
            "https://cache.test/",
            Destination::Empty,
            RequestMode::NoCors,
        );
        context.credentials = Credentials::Omit;
        let headers = vec![("Clear-Site-Data".into(), "\"cache\"".into())];
        cache.observe_response(&context, "https://cache.test/clear", &headers);
        assert!(cache
            .lookup("partition", "https://cache.test/", &BTreeMap::new())
            .is_some());
        context.credentials = Credentials::SameOrigin;
        cache.observe_response(&context, "https://cache.test/clear", &headers);
        cache.store(old);
        assert!(cache
            .lookup("partition", "https://cache.test/", &BTreeMap::new())
            .is_none());
    }
    #[test]
    fn freshness_accounts_for_date_age_expires_and_last_modified() {
        let now = SystemTime::now();
        let past = httpdate::fmt_http_date(now - Duration::from_secs(3600));
        assert!(entry(&[
            ("date", past.clone()),
            ("cache-control", "max-age=7200".into())
        ])
        .fresh());
        assert!(!entry(&[
            ("date", past.clone()),
            ("cache-control", "max-age=1800".into())
        ])
        .fresh());
        assert!(!entry(&[
            ("cache-control", "max-age=100".into()),
            ("age", "200".into())
        ])
        .fresh());
        assert!(entry(&[(
            "expires",
            httpdate::fmt_http_date(now + Duration::from_secs(3600))
        )])
        .fresh());
        assert!(!entry(&[("expires", past.clone())]).fresh());
        assert!(entry(&[
            ("date", past.clone()),
            (
                "last-modified",
                httpdate::fmt_http_date(now - Duration::from_secs(360000))
            )
        ])
        .fresh());
        assert!(!entry(&[
            ("date", past),
            ("cache-control", "max-age=7200, no-cache".into())
        ])
        .fresh());
    }
    #[test]
    fn validators_and_304_merge_keep_payload_and_replace_vary_fields() {
        let mut entry = entry(&[
            ("etag", "\"v1\"".into()),
            ("last-modified", "Wed, 14 Oct 2026 12:00:00 GMT".into()),
            ("x-old", "kept".into()),
            ("vary", "referer".into()),
        ]);
        let mut request = BTreeMap::from([("x-flavor".into(), "new".into())]);
        entry.validators(&mut request);
        assert_eq!(request["if-none-match"], "\"v1\"");
        assert_eq!(
            request["if-modified-since"],
            "Wed, 14 Oct 2026 12:00:00 GMT"
        );
        entry.merge(
            &[
                ("vary".into(), "x-flavor".into()),
                ("x-new".into(), "a".into()),
                ("x-new".into(), "b".into()),
                ("content-length".into(), "0".into()),
                ("set-cookie".into(), "ignore=1".into()),
            ],
            &request,
        );
        assert_eq!(entry.body, [0, 255]);
        assert_eq!(entry.status, 200);
        assert_eq!(values(&entry.headers, "x-old"), "kept");
        assert_eq!(values(&entry.headers, "x-new"), "a, b");
        assert_eq!(entry.vary, vec![("x-flavor".into(), Some("new".into()))]);
        assert_eq!(values(&entry.headers, "content-length"), "");
        assert_eq!(values(&entry.headers, "set-cookie"), "");
    }
}
