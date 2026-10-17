use super::*;

fn seeded() -> BrowserStorage {
    let storage = BrowserStorage::default();
    let mut areas = storage.0.lock().unwrap();
    for (id, partition, site, origin, ancestor, session) in [
        (1, 0, "https://top.test", "https://target.test", true, false),
        (2, 0, "https://top.test", "https://target.test", true, true),
        (3, 7, "https://top.test", "https://target.test", true, false),
        (
            4,
            0,
            "https://other.test",
            "https://target.test",
            true,
            false,
        ),
        (5, 0, "https://top.test", "https://other.test", true, false),
        (
            6,
            0,
            "https://top.test",
            "https://target.test",
            false,
            false,
        ),
    ] {
        areas.by_origin.insert(
            StorageKey {
                partition,
                site: site.into(),
                origin: origin.into(),
                cross_site_ancestor: ancestor,
                session,
                tab: 0,
            },
            id,
        );
        areas
            .values
            .insert(id, vec![(vec![0xd800, 0], vec![0xdc00])]);
    }
    drop(areas);
    storage
}
fn context() -> RequestContext {
    let mut context = RequestContext::resource(
        "https://target.test/",
        Destination::Empty,
        RequestMode::Cors,
    );
    context.top_level_source = Some("https://top.test/page".into());
    context.cookie_site_is_opaque = true;
    context
}
fn headers(value: &str) -> Vec<(String, String)> {
    vec![("Clear-Site-Data".into(), value.into())]
}

#[test]
fn clear_preserves_native_area_ids_and_other_partition_keys() {
    let storage = seeded();
    let before = storage.0.lock().unwrap().by_origin.clone();
    storage.clear_from_response(
        0,
        &context(),
        "https://target.test/clear",
        &headers("\"storage\""),
    );
    let areas = storage.0.lock().unwrap();
    assert_eq!(areas.by_origin, before);
    for id in 1..=6 {
        assert_eq!(areas.values[&id].is_empty(), id <= 2, "area{id}");
    }
}

#[test]
fn omitted_credentials_and_unquoted_or_cache_headers_preserve_storage() {
    for (header, credentials) in [
        ("\"storage\"", Credentials::Omit),
        ("storage", Credentials::Include),
        ("\"cache\"", Credentials::Include),
    ] {
        let storage = seeded();
        let mut context = context();
        context.credentials = credentials;
        storage.clear_from_response(0, &context, "https://target.test/clear", &headers(header));
        assert!(storage
            .0
            .lock()
            .unwrap()
            .values
            .values()
            .all(|value| !value.is_empty()));
    }
}

#[test]
fn clear_source_requires_https_or_loopback_and_expired_inbox_is_reclaimed() {
    for (target, expected) in [
        ("https://target.test", true),
        ("http://target.test", false),
        ("http://127.0.0.1", true),
        ("http://[::1]", true),
        ("http://localhost", true),
        ("http://local.test", false),
    ] {
        let storage = BrowserStorage::default();
        let site = turbo_surf_core::request_context::schemeful_site(target).unwrap();
        let origin = turbo_surf_core::request_context::origin(target).unwrap();
        let mut context = RequestContext::resource(target, Destination::Empty, RequestMode::Cors);
        context.top_level_source = Some(target.into());
        let inbox = std::sync::Arc::new(std::sync::Mutex::new(storage_events::Inbox::default()));
        {
            let mut areas = storage.0.lock().unwrap();
            areas.by_origin.insert(
                StorageKey {
                    partition: 0,
                    site,
                    origin,
                    cross_site_ancestor: false,
                    session: false,
                    tab: 0,
                },
                1,
            );
            areas.values.insert(1, vec![(vec![1], vec![2])]);
            areas.subscribers.push(std::sync::Arc::downgrade(&inbox));
        }
        drop(inbox);
        storage.clear_from_response(0, &context, target, &headers("\"*\""));
        let areas = storage.0.lock().unwrap();
        assert_eq!(areas.values[&1].is_empty(), expected, "{target}");
        if expected {
            assert_eq!(areas.subscribers.len(), 0);
        }
    }
}

#[test]
fn iframe_response_clears_the_resulting_cross_site_document_partition() {
    let storage = seeded();
    let mut context = context();
    context.destination = Destination::Iframe;
    context.cookie_site_is_opaque = false;
    storage.clear_from_response(
        0,
        &context,
        "https://target.test/clear",
        &headers("\"storage\""),
    );
    let areas = storage.0.lock().unwrap();
    assert_eq!(areas.values[&1].len(), 0);
    assert_eq!(areas.values[&2].len(), 0);
    assert_eq!(areas.values[&6].len(), 1);
}
#[test]
fn top_level_navigation_uses_the_new_site_and_no_cross_site_ancestor() {
    let storage = seeded();
    {
        let mut areas = storage.0.lock().unwrap();
        areas.by_origin.insert(
            StorageKey {
                partition: 0,
                site: "https://target.test".into(),
                origin: "https://target.test".into(),
                cross_site_ancestor: false,
                session: false,
                tab: 0,
            },
            7,
        );
        areas.values.insert(7, vec![(vec![1], vec![2])]);
    }
    let mut context = context();
    context.destination = Destination::Document;
    context.mode = RequestMode::Navigate;
    storage.clear_from_response(
        0,
        &context,
        "https://target.test/clear",
        &headers("\"storage\""),
    );
    let areas = storage.0.lock().unwrap();
    assert_eq!(areas.values[&7].len(), 0);
    for id in 1..=6 {
        assert_eq!(areas.values[&id].len(), 1, "area{id}");
    }
}

#[test]
fn response_observer_owns_only_native_storage_and_releases_it_on_drop() {
    let storage = seeded();
    let weak = std::sync::Arc::downgrade(&storage.0);
    let observer = storage.response_storage_effects();
    drop(storage);
    assert_eq!(weak.strong_count(), 1);
    observer(
        &context(),
        "https://target.test/clear",
        &headers("\"storage\""),
    );
    assert_eq!(weak.upgrade().unwrap().lock().unwrap().values[&1].len(), 0);
    drop(observer);
    assert_eq!(weak.strong_count(), 0);
}
