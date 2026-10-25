//! The compiled registry against the default release's measured identity fixture
//! (harness/detection/fixtures/chrome-identity-<DEFAULT_MAJOR>.json). Nothing here
//! names a Chrome version: a re-version regenerates the data, not this test.
use serde_json::Value;
use turbo_surf_core::{browser, fingerprint, request_context};

fn fixture() -> Value {
    let path = format!(
        "{}/../../../harness/detection/fixtures/chrome-identity-{}.json",
        env!("CARGO_MANIFEST_DIR"),
        browser::DEFAULT_MAJOR
    );
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
}

fn header<'a>(request: &'a Value, name: &str) -> Option<&'a str> {
    request["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0].as_str().unwrap().eq_ignore_ascii_case(name))
        .map(|pair| pair[1].as_str().unwrap())
}

fn chrome_request<'a>(fixture: &'a Value, kind: &str) -> &'a Value {
    fixture["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["kind"] == kind && request["origin"] != "B")
        .unwrap_or_else(|| panic!("fixture has no {kind} request"))
}

#[test]
fn default_release_identity_is_the_measured_one() {
    let fixture = fixture();
    let release = browser::default_release();
    let profile = fingerprint::default_profile();
    let js = &fixture["js"];
    assert_eq!(
        fixture["browser"]["product"],
        format!("Chrome/{}", release.full_version)
    );
    assert_eq!(
        js["userAgentData"]["high"]["uaFullVersion"],
        release.full_version
    );
    assert_eq!(js["navigator"]["userAgent"], profile.user_agent);
    assert_eq!(
        js["navigator"]["deviceMemory"],
        release.default_device_memory
    );
    let nav = chrome_request(&fixture, "nav");
    assert_eq!(header(nav, "user-agent"), Some(profile.user_agent.as_str()));
    assert_eq!(header(nav, "sec-ch-ua"), Some(profile.sec_ch_ua.as_str()));
    assert_eq!(header(nav, "accept"), Some(release.navigation_accept));
    let names: Vec<String> = nav["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| pair[0].as_str().unwrap().to_ascii_lowercase())
        .filter(|name| !name.starts_with(':'))
        .collect();
    let expected: Vec<&str> = release
        .header_order
        .iter()
        .copied()
        .filter(|name| *name != "cookie")
        .collect();
    assert_eq!(names, expected);
    let hinted = chrome_request(&fixture, "ch-next");
    assert_eq!(
        header(hinted, "sec-ch-ua-full-version-list"),
        Some(fingerprint::brand_header(release.major, release.full_version, true).as_str())
    );
}

#[test]
fn subresource_metadata_comes_from_the_measured_release() {
    use request_context::{Destination, RequestContext, RequestMode};
    let fixture = fixture();
    let release = browser::default_release();
    for (kind, destination, mode) in [
        ("img", Destination::Image, RequestMode::NoCors),
        ("style", Destination::Style, RequestMode::NoCors),
        ("script", Destination::Script, RequestMode::NoCors),
        ("fetch", Destination::Empty, RequestMode::Cors),
    ] {
        let measured = chrome_request(&fixture, kind);
        let context = RequestContext::resource("https://a.test/page", destination, mode);
        let built = context.headers("https://a.test/r", "GET");
        assert_eq!(
            Some(built["accept"].as_str()),
            header(measured, "accept"),
            "{kind}"
        );
        assert_eq!(
            Some(built["priority"].as_str()),
            header(measured, "priority"),
            "{kind}"
        );
        let class =
            browser::request_class("GET", |name| header(measured, name).map(str::to_string));
        let order = release.request_order(&class).expect("measured class order");
        let measured_names: Vec<String> = measured["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| pair[0].as_str().unwrap().to_ascii_lowercase())
            .filter(|name| !name.starts_with(':'))
            .collect();
        let positions: Vec<usize> = measured_names
            .iter()
            .map(|name| {
                order
                    .iter()
                    .position(|entry| entry.eq_ignore_ascii_case(name))
                    .unwrap_or_else(|| panic!("{kind}: {name} missing from {class} order"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{kind} order"
        );
    }
}

#[test]
fn every_release_stays_selectable() {
    for release in browser::RELEASES {
        let profile = (0..fingerprint::pool_size())
            .map(fingerprint::profile_at)
            .find(|profile| profile.chrome_major == release.major)
            .expect("each release appears in the seed pool");
        assert_eq!(profile.full_version, release.full_version);
        assert!(profile
            .user_agent
            .contains(&format!("Chrome/{}.0.0.0", release.major)));
    }
}
