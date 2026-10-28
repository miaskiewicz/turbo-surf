use super::*;
use crate::request_context::Credentials;
use std::sync::atomic::AtomicBool;

const DICTIONARY: &[u8] = include_bytes!(
    "../../../../../harness/detection/fixtures/compression-dictionary/dictionary.bin"
);
const DECODED: &[u8] =
    include_bytes!("../../../../../harness/detection/fixtures/compression-dictionary/decoded.bin");
const DCB: &[u8] =
    include_bytes!("../../../../../harness/detection/fixtures/compression-dictionary/encoded.dcb");
const DCZ: &[u8] =
    include_bytes!("../../../../../harness/detection/fixtures/compression-dictionary/encoded.dcz");

fn fixture_dictionary() -> SelectedDictionary {
    let dictionaries = DictionaryStore::default();
    assert!(store(&dictionaries, "dict", r#"match="/*""#, DICTIONARY));
    dictionaries
        .select("partition", "https://dict.test/result", &context())
        .unwrap()
}

#[test]
fn independently_encoded_official_dictionary_fixtures_are_pinned_and_require_dictionary() {
    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../harness/detection/fixtures/compression-dictionary/provenance.json"
    ))
    .unwrap();
    for (name, bytes) in [
        ("dictionary.bin", DICTIONARY),
        ("decoded.bin", DECODED),
        ("encoded.dcb", DCB),
        ("encoded.dcz", DCZ),
    ] {
        assert_eq!(
            provenance["files"][name]["sha256"],
            format!("{:x}", Sha256::digest(bytes))
        );
    }
    assert_eq!(
        provenance["generatorSha256"],
        format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../../../../harness/detection/generate-compression-dictionary-fixtures.py"
            ))
        )
    );
    assert_eq!(
        provenance["zstdProducerSha256"],
        format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../../../../harness/detection/fixtures/compression-dictionary/zstd-encoder.c"
            ))
        )
    );
    for (encoding, bytes) in [("dcb", DCB), ("dcz", DCZ)] {
        let decoded = decode::decode_sync(
            encoding,
            bytes.to_vec(),
            fixture_dictionary(),
            DECODED.len(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(decoded, DECODED, "{encoding}");
        let mut missing = fixture_dictionary();
        missing.body = Arc::new(Vec::new());
        assert!(
            match decode::decode_sync(
                encoding,
                bytes.to_vec(),
                missing,
                DECODED.len(),
                Arc::new(AtomicBool::new(false))
            ) {
                Err(_) => true,
                Ok(decoded) => decoded != DECODED,
            },
            "{encoding} must actually require its dictionary"
        );
    }
}

#[test]
fn dictionary_codecs_reject_bad_envelope_hash_truncation_output_bombs_and_cancellation() {
    for (encoding, bytes) in [("dcb", DCB), ("dcz", DCZ)] {
        for index in [0, 10] {
            let mut invalid = bytes.to_vec();
            invalid[index] ^= 1;
            assert!(decode::decode_sync(
                encoding,
                invalid,
                fixture_dictionary(),
                DECODED.len(),
                Arc::new(AtomicBool::new(false))
            )
            .is_err());
        }
        assert!(decode::decode_sync(
            encoding,
            bytes[..12].to_vec(),
            fixture_dictionary(),
            DECODED.len(),
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        assert!(matches!(
            decode::decode_sync(
                encoding,
                bytes.to_vec(),
                fixture_dictionary(),
                DECODED.len() - 1,
                Arc::new(AtomicBool::new(false))
            ),
            Err(decode::DecodeError::Size)
        ));
        assert!(matches!(
            decode::decode_sync(
                encoding,
                bytes.to_vec(),
                fixture_dictionary(),
                DECODED.len(),
                Arc::new(AtomicBool::new(true))
            ),
            Err(decode::DecodeError::Cancelled)
        ));
    }
}

#[test]
fn official_extended_brotli_window_above_sixteen_mib_is_rejected_before_decode() {
    let bytes = include_bytes!(
        "../../../../../harness/detection/fixtures/compression-dictionary/oversized-window.dcb"
    );
    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../harness/detection/fixtures/compression-dictionary/provenance.json"
    ))
    .unwrap();
    assert_eq!(
        provenance["files"]["oversized-window.dcb"]["sha256"],
        format!("{:x}", Sha256::digest(bytes))
    );
    assert!(
        matches!(decode::decode_sync("dcb",bytes.to_vec(),fixture_dictionary(),DECODED.len(),Arc::new(AtomicBool::new(false))),Err(decode::DecodeError::Invalid(message)) if message.contains("window"))
    );
}

fn context() -> RequestContext {
    RequestContext::resource(
        "https://dict.test/page",
        Destination::Script,
        RequestMode::Cors,
    )
}
fn headers(value: &str) -> Vec<(String, String)> {
    vec![
        ("Use-As-Dictionary".into(), value.into()),
        ("Cache-Control".into(), "max-age=60".into()),
    ]
}
fn store(store: &DictionaryStore, path: &str, options: &str, bytes: &[u8]) -> bool {
    store.store_response(
        "partition",
        &format!("https://dict.test/{path}"),
        &context(),
        200,
        &headers(options),
        bytes,
    )
}

#[test]
fn structured_fields_and_real_url_patterns_match_destinations_and_origin() {
    let dictionaries = DictionaryStore::default();
    assert!(store(
        &dictionaries,
        "dict",
        r#"match="/app/:id", match-dest=("script"), id="authored\"id", type=raw"#,
        b"authored dictionary"
    ));
    let selected = dictionaries
        .select("partition", "https://dict.test/app/main.js", &context())
        .unwrap();
    let mut outgoing = BTreeMap::new();
    selected.negotiate(&mut outgoing);
    assert_eq!(outgoing["dictionary-id"], r#""authored\"id""#);
    assert_eq!(
        outgoing["accept-encoding"],
        "gzip, deflate, br, zstd, dcb, dcz"
    );
    assert!(dictionaries
        .select("other", "https://dict.test/app/main.js", &context())
        .is_none());
    assert!(dictionaries
        .select("partition", "https://other.test/app/main.js", &context())
        .is_none());
    let mut wrong_destination = context();
    wrong_destination.destination = Destination::Style;
    assert!(dictionaries
        .select(
            "partition",
            "https://dict.test/app/main.js",
            &wrong_destination
        )
        .is_none());
}

#[test]
fn dictionary_selection_uses_destination_then_pattern_length_then_fetch_order() {
    let dictionaries = DictionaryStore::default();
    assert!(store(
        &dictionaries,
        "general",
        r#"match="/app/*""#,
        b"general"
    ));
    assert!(store(
        &dictionaries,
        "specific",
        r#"match="/app/main.js""#,
        b"specific"
    ));
    assert!(store(
        &dictionaries,
        "destination",
        r#"match="/*", match-dest=("script")"#,
        b"destination"
    ));
    assert_eq!(
        dictionaries
            .select("partition", "https://dict.test/app/main.js", &context())
            .unwrap()
            .body
            .as_slice(),
        b"destination"
    );
    assert!(store(
        &dictionaries,
        "later",
        r#"match="/*", match-dest=("script")"#,
        b"later"
    ));
    assert_eq!(
        dictionaries
            .select("partition", "https://dict.test/app/main.js", &context())
            .unwrap()
            .body
            .as_slice(),
        b"later"
    );
}

#[test]
fn malformed_metadata_and_regular_expression_patterns_never_publish() {
    for value in [
        r#"id="missing-match""#,
        r#"match="/*",type=unsupported"#,
        r#"match="/*",type="raw""#,
        r#"match="/*",match-dest=(script)"#,
        r#"match="/app/(foo|bar)""#,
        r#"match="/*",id=?1"#,
        r#"match="/*" trailing"#,
    ] {
        let dictionaries = DictionaryStore::default();
        assert!(!store(&dictionaries, "dict", value, b"authored"), "{value}");
        assert!(dictionaries
            .select("partition", "https://dict.test/app/main.js", &context())
            .is_none());
    }
    assert!(!store(
        &DictionaryStore::default(),
        "dict",
        &format!("match=\"/*\",id=\"{}\"", "x".repeat(1025)),
        b"authored"
    ));
}

#[test]
fn cache_freshness_no_store_age_and_clear_epochs_are_enforced() {
    for cache_control in ["max-age=0", "max-age=60, no-store", "max-age=60, no-cache"] {
        let dictionaries = DictionaryStore::default();
        let mut metadata = headers(r#"match="/*""#);
        metadata[1].1 = cache_control.into();
        assert!(!dictionaries.store_response(
            "partition",
            "https://dict.test/dict",
            &context(),
            200,
            &metadata,
            b"authored"
        ));
    }
    let dictionaries = DictionaryStore::default();
    let mut metadata = headers(r#"match="/*""#);
    metadata.push(("Age".into(), "61".into()));
    assert!(!dictionaries.store_response(
        "partition",
        "https://dict.test/dict",
        &context(),
        200,
        &metadata,
        b"authored"
    ));
    assert!(!dictionaries.store_generation(
        dictionaries.generation(),
        "partition",
        "https://dict.test/delayed",
        &context(),
        200,
        &headers(r#"match="/*""#),
        b"authored",
        SystemTime::now() - std::time::Duration::from_secs(120),
    ));
    let generation = dictionaries.generation();
    dictionaries.clear();
    assert!(!dictionaries.store_generation(
        generation,
        "partition",
        "https://dict.test/dict",
        &context(),
        200,
        &headers(r#"match="/*""#),
        b"authored",
        SystemTime::now(),
    ));
    assert!(store(&dictionaries, "dict", r#"match="/*""#, b"authored"));
    dictionaries.clear_partition("other");
    assert!(dictionaries
        .select("partition", "https://dict.test/app", &context())
        .is_some());
    dictionaries.clear_origin("https://dict.test/clear");
    assert!(dictionaries
        .select("partition", "https://dict.test/app", &context())
        .is_none());
}

#[test]
fn cross_origin_dictionary_requires_actual_cors_admission() {
    let dictionaries = DictionaryStore::default();
    let mut request = RequestContext::resource(
        "https://parent.test/",
        Destination::Empty,
        RequestMode::Cors,
    );
    request.credentials = Credentials::Omit;
    let mut metadata = headers(r#"match="/*""#);
    assert!(!dictionaries.store_response(
        "partition",
        "https://dict.test/dict",
        &request,
        200,
        &metadata,
        b"authored"
    ));
    metadata.push(("Access-Control-Allow-Origin".into(), "*".into()));
    assert!(dictionaries.store_response(
        "partition",
        "https://dict.test/dict",
        &request,
        200,
        &metadata,
        b"authored"
    ));
    request.mode = RequestMode::NoCors;
    assert!(!dictionaries.store_response(
        "partition",
        "https://dict.test/dict",
        &request,
        200,
        &metadata,
        b"opaque"
    ));
    request.mode = RequestMode::Cors;
    request.opaque_origin = true;
    assert!(!dictionaries.store_response(
        "partition",
        "https://dict.test/dict",
        &request,
        200,
        &metadata,
        b"opaque"
    ));
}

#[test]
fn http_cache_clones_share_dictionary_state_and_origin_clear() {
    let cache = crate::http_cache::HttpCache::default();
    let clone = cache.clone();
    assert!(store(
        cache.dictionary_store(),
        "dict",
        r#"match="/*""#,
        b"authored"
    ));
    assert!(clone
        .dictionary_store()
        .select("partition", "https://dict.test/app", &context())
        .is_some());
    clone.clear_response("https://dict.test/clear", r#""cache""#);
    assert!(cache
        .dictionary_store()
        .select("partition", "https://dict.test/app", &context())
        .is_none());
}

#[test]
fn relative_patterns_use_dictionary_base_as_independent_chrome_does() {
    let dictionaries = DictionaryStore::default();
    assert!(store(
        &dictionaries,
        "dictionary",
        r#"match="target""#,
        b"authored"
    ));
    assert!(dictionaries
        .select("partition", "https://dict.test/target", &context())
        .is_some());
    assert!(dictionaries
        .select("partition", "https://dict.test/sub/target", &context())
        .is_none());
}

#[test]
fn dictionary_transport_requires_https_even_for_trustworthy_http_loopback() {
    let dictionaries = DictionaryStore::default();
    let request = RequestContext::resource(
        "http://127.0.0.1/page",
        Destination::Empty,
        RequestMode::Cors,
    );
    assert!(!dictionaries.store_response(
        "partition",
        "http://127.0.0.1/dictionary",
        &request,
        200,
        &headers(r#"match="/*""#),
        b"authored"
    ));
    assert!(dictionaries
        .select("partition", "http://127.0.0.1/target", &request)
        .is_none());
}

#[tokio::test]
async fn dictionary_decoder_admission_is_bounded_and_cancelled_waiters_release_capacity() {
    let _serial = decode::TEST_SERIAL.lock().await;
    let first = decode::reserve().await.unwrap();
    let second = decode::reserve().await.unwrap();
    let mut blocked = Box::pin(decode::reserve());
    assert!(futures_util::poll!(&mut blocked).is_pending());
    drop(blocked);
    let mut next = Box::pin(decode::reserve());
    assert!(futures_util::poll!(&mut next).is_pending());
    drop(first);
    let admitted = next.await.unwrap();
    drop(second);
    drop(admitted);
    let first = decode::reserve().await.unwrap();
    let second = decode::reserve().await.unwrap();
    drop((first, second));
}

#[test]
fn dictionary_storage_rejects_oversize_entries_and_evicts_under_both_cache_bounds() {
    let dictionaries = DictionaryStore::default();
    assert!(!store(
        &dictionaries,
        "oversize",
        r#"match="/*""#,
        &vec![0; MAX_DICTIONARY_BYTES + 1],
    ));
    let body = vec![0; MAX_DICTIONARY_BYTES];
    for index in 0..5 {
        assert!(store(
            &dictionaries,
            &format!("dictionary-{index}"),
            &format!(r#"match="/result-{index}""#),
            &body,
        ));
    }
    assert!(dictionaries
        .select("partition", "https://dict.test/result-0", &context())
        .is_none());
    assert!(dictionaries
        .select("partition", "https://dict.test/result-4", &context())
        .is_some());
    dictionaries.clear();
    for index in 0..65 {
        assert!(store(
            &dictionaries,
            &format!("dictionary-{index}"),
            &format!(r#"match="/result-{index}""#),
            b"small",
        ));
    }
    assert!(dictionaries
        .select("partition", "https://dict.test/result-0", &context())
        .is_none());
    assert!(dictionaries
        .select("partition", "https://dict.test/result-64", &context())
        .is_some());
}

#[test]
fn use_as_dictionary_rejects_malformed_or_unsupported_fields() {
    let dictionaries = DictionaryStore::default();
    for bad in [
        r#"match=("/a")"#,                    // match must be a string item
        r#"match="/(\\d+)""#,                 // regexp groups are not allowed
        r#"match="/*", type=br"#,             // only type=raw
        r#"match="/*", type="raw""#,          // type must be a token
        r#"match="/*", id=7"#,                // id must be a string
        r#"match="/*", match-dest="script""#, // match-dest must be an inner list
        r#"match="/*", match-dest=(script)"#, // of strings
        "match=",                             // not a structured dictionary
    ] {
        assert!(!store(&dictionaries, "d", bad, b"x"), "{bad}");
    }
    let long_id = format!(r#"match="/*", id="{}""#, "i".repeat(1025));
    assert!(!store(&dictionaries, "d", &long_id, b"x"));
    let huge = format!(r#"match="/*", id="{}""#, "i".repeat(16 * 1024));
    assert!(!store(&dictionaries, "d", &huge, b"x"));
    assert!(dictionaries
        .select("partition", "https://dict.test/a", &context())
        .is_none());
    // An exactly 1024-byte id is accepted.
    let max_id = format!(r#"match="/*", id="{}""#, "i".repeat(1024));
    assert!(store(&dictionaries, "d", &max_id, b"x"));
}

#[test]
fn match_patterns_new_urlpattern_rejects_never_publish() {
    // Each of these throws a TypeError from `new URLPattern(match, dictionaryURL)`
    // in Chrome 156 (checked with that base URL), so the response is not
    // registered as a dictionary. (`/a)` is accepted there, as here.)
    let dictionaries = DictionaryStore::default();
    for bad in [
        r#"match="/{""#,           // unterminated group
        r#"match="/:""#,           // parameter without a name
        r#"match="/(""#,           // unterminated regexp
        r#"match="/a}""#,          // unbalanced brace
        r#"match="https://[:1/""#, // invalid IPv6 hostname pattern
        r#"match="/:a/:a""#,       // duplicate group name
    ] {
        assert!(!store(&dictionaries, "d", bad, b"x"), "{bad}");
    }
    assert!(dictionaries
        .select("partition", "https://dict.test/a", &context())
        .is_none());
}

#[test]
fn a_streamed_dictionary_body_is_capped_at_eight_mib() {
    let dictionaries = DictionaryStore::default();
    let mut pending = PendingDictionary::new(
        &dictionaries,
        dictionaries.generation(),
        "partition",
        "https://dict.test/d",
        &context(),
        200,
        headers(r#"match="/*""#),
        SystemTime::now(),
    )
    .unwrap();
    assert!(pending.append(&vec![1; MAX_DICTIONARY_BYTES - 1]));
    assert!(!pending.append(&[2, 3]));
    assert!(pending.append(&[4]));
    assert!(!pending.append(&[5]));
    pending.commit();
    let selected = dictionaries
        .select("partition", "https://dict.test/a", &context())
        .unwrap();
    assert_eq!(selected.body.len(), MAX_DICTIONARY_BYTES);
    assert_eq!(selected.body[MAX_DICTIONARY_BYTES - 1], 4);
    // A response without Use-As-Dictionary never starts a pending dictionary.
    assert!(PendingDictionary::new(
        &dictionaries,
        0,
        "partition",
        "https://dict.test/d",
        &context(),
        200,
        vec![("Cache-Control".into(), "max-age=60".into())],
        SystemTime::now(),
    )
    .is_none());
}

#[test]
fn dictionaries_are_admitted_only_from_secure_untainted_same_origin_or_cors_responses() {
    let dictionaries = DictionaryStore::default();
    let put = |url: &str, ctx: &RequestContext| {
        dictionaries.store_response("partition", url, ctx, 200, &headers(r#"match="/*""#), b"x")
    };
    assert!(!put("http://dict.test/d", &context()));
    let mut opaque = context();
    opaque.opaque_origin = true;
    assert!(!put("https://dict.test/d", &opaque));
    let mut tainted = context();
    tainted.origin_tainted = true;
    assert!(!put("https://dict.test/d", &tainted));
    let mut sourceless = context();
    sourceless.source = None;
    assert!(put("https://dict.test/d", &sourceless));
    // Non-200 responses and no-store responses are never dictionaries.
    assert!(!dictionaries.store_response(
        "p",
        "https://dict.test/d",
        &context(),
        206,
        &headers(r#"match="/*""#),
        b"x"
    ));
    let no_store = vec![
        ("Use-As-Dictionary".to_string(), r#"match="/*""#.to_string()),
        (
            "Cache-Control".to_string(),
            "max-age=60, no-store".to_string(),
        ),
    ];
    assert!(!dictionaries.store_response(
        "p",
        "https://dict.test/d",
        &context(),
        200,
        &no_store,
        b"x"
    ));
}

#[test]
fn every_destination_has_its_fetch_dest_name() {
    let names: Vec<&str> = [
        Destination::Document,
        Destination::Iframe,
        Destination::Script,
        Destination::Style,
        Destination::Image,
        Destination::Empty,
        Destination::Report,
    ]
    .into_iter()
    .map(|d| {
        let mut c = context();
        c.destination = d;
        destination(&c)
    })
    .collect();
    assert_eq!(
        names,
        ["document", "iframe", "script", "style", "image", "", "report"]
    );
}
