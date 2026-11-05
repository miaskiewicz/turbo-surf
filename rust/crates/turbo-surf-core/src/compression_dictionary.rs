//! Session-owned RFC 9842 dictionaries. Only complete, policy-admitted responses publish.
use crate::request_context::{origin, Destination, RequestContext, RequestMode};
use base64::Engine;
use sfv::{BareItem, Dictionary, ListEntry, Parser};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use url::Url;
use urlpattern::{UrlPattern, UrlPatternInit, UrlPatternMatchInput};

pub(crate) mod decode;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod tests;

pub(crate) const MAX_DICTIONARY_BYTES: usize = 8 * 1024 * 1024;
const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DICTIONARIES: usize = 64;

#[derive(Clone, Default)]
pub struct DictionaryStore(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    generation: u64,
}
struct Entry {
    partition: String,
    url: String,
    origin: String,
    pattern: UrlPattern,
    match_length: usize,
    destinations: Vec<String>,
    headers: Vec<(String, String)>,
    stored: SystemTime,
    dictionary: SelectedDictionary,
}

impl State {
    /// Replace any entry for the same partition+URL, then evict oldest-first until
    /// the new entry fits the count and byte budgets.
    fn insert(&mut self, entry: Entry) {
        self.entries
            .retain(|old| !(old.partition == entry.partition && old.url == entry.url));
        let incoming = entry.dictionary.body.len();
        while self.entries.len() >= MAX_DICTIONARIES
            || self
                .entries
                .iter()
                .map(|old| old.dictionary.body.len())
                .sum::<usize>()
                + incoming
                > MAX_STORE_BYTES
        {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }
}

/// The response-level admission gate: a complete, bounded, cacheable 200.
fn storable_response(
    status: u16,
    url: &str,
    context: &RequestContext,
    headers: &[(String, String)],
    body: &[u8],
) -> bool {
    status == 200
        && body.len() <= MAX_DICTIONARY_BYTES
        && admitted(url, context, headers)
        && !values(headers, "cache-control")
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("no-store"))
}

/// The parsed `Use-As-Dictionary` members a stored entry keeps.
struct DictionaryFields {
    pattern: UrlPattern,
    match_length: usize,
    id: String,
    destinations: Vec<String>,
}

fn parse_use_as_dictionary(url: &str, headers: &[(String, String)]) -> Option<DictionaryFields> {
    // admitted() already required `url` to parse as an https URL.
    let base = Url::parse(url).expect("admitted() parsed the dictionary URL");
    let input = values(headers, "use-as-dictionary");
    if input.len() > 16 * 1024 {
        return None;
    }
    let fields = Parser::new(&input).parse::<Dictionary>().ok()?;
    let pattern_string = string_member(&fields, "match")?;
    let pattern = match_pattern(pattern_string, base)?;
    if !type_is_raw(&fields) {
        return None;
    }
    Some(DictionaryFields {
        pattern,
        match_length: pattern_string.len(),
        id: dictionary_id(&fields)?.to_owned(),
        destinations: match_destinations(&fields)?,
    })
}

/// The `match` URL pattern; regexp groups are not allowed.
fn match_pattern(pattern_string: &str, base: Url) -> Option<UrlPattern> {
    let init = UrlPatternInit::parse_constructor_string::<regex::Regex>(pattern_string, Some(base))
        .ok()?;
    let pattern = UrlPattern::parse(init, Default::default()).ok()?;
    (!pattern.has_regexp_groups()).then_some(pattern)
}

/// An absent `type` defaults to raw; a present one must be the token `raw`.
fn type_is_raw(fields: &Dictionary) -> bool {
    fields.get("type").is_none_or(|kind| {
        matches!(kind, ListEntry::Item(item) if matches!(&item.bare_item, BareItem::Token(token) if token.as_str() == "raw"))
    })
}

/// An absent `id` is empty; a present one must be a string of at most 1024 bytes.
fn dictionary_id(fields: &Dictionary) -> Option<&str> {
    match fields.get("id") {
        None => Some(""),
        Some(_) => string_member(fields, "id").filter(|value| value.len() <= 1024),
    }
}

/// `match-dest` must be an inner list of strings when present.
fn match_destinations(fields: &Dictionary) -> Option<Vec<String>> {
    match fields.get("match-dest") {
        None => Some(Vec::new()),
        Some(ListEntry::InnerList(list)) => list
            .items
            .iter()
            .map(|item| match &item.bare_item {
                BareItem::String(value) => Some(value.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

#[derive(Clone)]
pub struct SelectedDictionary {
    pub(crate) body: Arc<Vec<u8>>,
    pub(crate) hash: [u8; 32],
    id: String,
}
#[derive(Clone)]
pub(crate) struct PendingDictionary {
    store: DictionaryStore,
    generation: u64,
    partition: String,
    url: String,
    context: RequestContext,
    status: u16,
    headers: Vec<(String, String)>,
    stored: SystemTime,
    body: Vec<u8>,
}
impl PendingDictionary {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: &DictionaryStore,
        generation: u64,
        partition: &str,
        url: &str,
        context: &RequestContext,
        status: u16,
        headers: Vec<(String, String)>,
        stored: SystemTime,
    ) -> Option<Self> {
        (!values(&headers, "use-as-dictionary").is_empty()).then(|| Self {
            store: store.clone(),
            generation,
            partition: partition.to_owned(),
            url: url.to_owned(),
            context: context.clone(),
            status,
            headers,
            stored,
            body: Vec::new(),
        })
    }
    pub(crate) fn append(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > MAX_DICTIONARY_BYTES.saturating_sub(self.body.len()) {
            return false;
        }
        self.body.extend_from_slice(bytes);
        true
    }
    pub(crate) fn commit(self) {
        self.store.store_generation(
            self.generation,
            &self.partition,
            &self.url,
            &self.context,
            self.status,
            &self.headers,
            &self.body,
            self.stored,
        );
    }
}
impl SelectedDictionary {
    /// These codings are inserted only after selecting a genuine scoped dictionary.
    pub(crate) fn negotiate(&self, headers: &mut BTreeMap<String, String>) {
        // A Range request keeps identity and still advertises the dictionary
        // codings (Chrome 156 control `header-range-request`).
        let base = if headers.contains_key("range") {
            "identity"
        } else {
            "gzip, deflate, br, zstd"
        };
        headers.insert("accept-encoding".into(), format!("{base}, dcb, dcz"));
        headers.insert(
            "available-dictionary".into(),
            format!(
                ":{}:",
                base64::engine::general_purpose::STANDARD.encode(self.hash)
            ),
        );
        if !self.id.is_empty() {
            headers.insert(
                "dictionary-id".into(),
                format!("\"{}\"", self.id.replace('\\', "\\\\").replace('"', "\\\"")),
            );
        }
    }
}

fn values(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
fn string_member<'a>(fields: &'a Dictionary, key: &str) -> Option<&'a str> {
    match fields.get(key)? {
        ListEntry::Item(item) => match &item.bare_item {
            BareItem::String(value) => Some(value.as_str()),
            _ => None,
        },
        _ => None,
    }
}
fn destination(context: &RequestContext) -> &'static str {
    match context.destination {
        Destination::Document => "document",
        Destination::Iframe => "iframe",
        Destination::Script => "script",
        Destination::Style => "style",
        Destination::Image => "image",
        Destination::Font => "font",
        Destination::Empty => "",
        Destination::Report => "report",
    }
}
fn admitted(url: &str, context: &RequestContext, headers: &[(String, String)]) -> bool {
    if !Url::parse(url).is_ok_and(|url| url.scheme() == "https")
        || context.opaque_origin
        || context.origin_tainted
    {
        return false;
    }
    let Some(source) = context.source.as_deref() else {
        return true;
    };
    if origin(source) == origin(url) {
        return true;
    }
    if context.mode != RequestMode::Cors {
        return context.mode == RequestMode::Navigate;
    }
    let allowed = values(headers, "access-control-allow-origin");
    let credentials = context.sends_credentials(url);
    (allowed == origin(source).unwrap_or_default() || allowed == "*" && !credentials)
        && (!credentials || values(headers, "access-control-allow-credentials") == "true")
}

impl DictionaryStore {
    pub(crate) fn generation(&self) -> u64 {
        self.0.lock().unwrap().generation
    }
    /// The caller must have completed its response-policy checks and the complete body.
    pub fn store_response(
        &self,
        partition: &str,
        url: &str,
        context: &RequestContext,
        status: u16,
        headers: &[(String, String)],
        body: &[u8],
    ) -> bool {
        self.store_generation(
            self.generation(),
            partition,
            url,
            context,
            status,
            headers,
            body,
            SystemTime::now(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_generation(
        &self,
        generation: u64,
        partition: &str,
        url: &str,
        context: &RequestContext,
        status: u16,
        headers: &[(String, String)],
        body: &[u8],
        stored: SystemTime,
    ) -> bool {
        if !storable_response(status, url, context, headers, body) {
            return false;
        }
        let Some(fields) = parse_use_as_dictionary(url, headers) else {
            return false;
        };
        if !crate::http_cache::fresh_headers(headers, stored) {
            return false;
        }
        let mut state = self.0.lock().unwrap();
        if generation != state.generation {
            return false;
        }
        state.insert(Entry {
            partition: partition.to_owned(),
            url: url.to_owned(),
            origin: origin(url).unwrap(),
            pattern: fields.pattern,
            match_length: fields.match_length,
            destinations: fields.destinations,
            headers: headers.to_vec(),
            stored,
            dictionary: SelectedDictionary {
                body: Arc::new(body.to_vec()),
                hash: Sha256::digest(body).into(),
                id: fields.id,
            },
        });
        true
    }
    pub fn select(
        &self,
        partition: &str,
        url: &str,
        context: &RequestContext,
    ) -> Option<SelectedDictionary> {
        if !Url::parse(url).is_ok_and(|url| url.scheme() == "https")
            || context.opaque_origin
            || context.origin_tainted
        {
            return None;
        }
        let url = Url::parse(url).ok()?;
        let origin = url.origin().ascii_serialization();
        let mut state = self.0.lock().unwrap();
        state
            .entries
            .retain(|entry| crate::http_cache::fresh_headers(&entry.headers, entry.stored));
        state
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.partition == partition
                    && entry.origin == origin
                    && (entry.destinations.is_empty()
                        || entry
                            .destinations
                            .iter()
                            .any(|value| value == destination(context)))
                    && entry
                        .pattern
                        .test(UrlPatternMatchInput::Url(url.clone()))
                        .unwrap_or(false)
            })
            .max_by_key(|(index, entry)| {
                (!entry.destinations.is_empty(), entry.match_length, *index)
            })
            .map(|(_, entry)| entry.dictionary.clone())
    }
    pub fn clear(&self) {
        let mut state = self.0.lock().unwrap();
        state.entries.clear();
        state.generation = state.generation.wrapping_add(1);
    }
    pub fn clear_origin(&self, url: &str) {
        let origin = origin(url);
        let mut state = self.0.lock().unwrap();
        state
            .entries
            .retain(|entry| Some(&entry.origin) != origin.as_ref());
        state.generation = state.generation.wrapping_add(1);
    }
    pub fn clear_partition(&self, partition: &str) {
        let mut state = self.0.lock().unwrap();
        state.entries.retain(|entry| entry.partition != partition);
        state.generation = state.generation.wrapping_add(1);
    }
}
