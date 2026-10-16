#[path = "../src/form_navigation.rs"]
mod form_navigation;

#[test]
fn actual_chrome_form_wire_encodings_preserve_names_duplicates_and_fragments() {
    for (fixture, encoding) in [
        (
            include_str!("../../../../harness/detection/fixtures/form-wire-utf8-chrome155.json"),
            encoding_rs::UTF_8,
        ),
        (
            include_str!(
                "../../../../harness/detection/fixtures/form-wire-windows1252-chrome155.json"
            ),
            encoding_rs::WINDOWS_1252,
        ),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
        for case in fixture["result"].as_array().unwrap() {
            let entries: Vec<(String, String)> =
                serde_json::from_value(case["entries"].clone()).unwrap();
            let plan = form_navigation::prepare(
                "http://local.invalid/start",
                "http://local.invalid/wire?old=1#retained",
                case["method"].as_str().unwrap(),
                case["enctype"].as_str().unwrap(),
                "_self",
                &entries,
                &[],
                encoding,
            )
            .unwrap();
            let url = url::Url::parse(&plan.url).unwrap();
            assert_eq!(
                format!(
                    "{}{}",
                    url.path(),
                    url.query()
                        .map_or(String::new(), |query| format!("?{query}"))
                ),
                case["wire"]["url"]
            );
            assert_eq!(format!("#{}", url.fragment().unwrap()), case["fragment"]);
            assert_eq!(plan.method, case["wire"]["method"]);
            assert_eq!(
                plan.headers.get("content-type").map(String::as_str),
                case["wire"]["contentType"].as_str()
            );
            assert_eq!(
                String::from_utf8_lossy(plan.body.as_deref().unwrap_or_default()),
                case["wire"]["body"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn only_native_hidden_charset_indices_are_replaced() {
    let entries = vec![
        ("_charset_".into(), "original".into()),
        ("_charset_".into(), "text".into()),
    ];
    let plan = form_navigation::prepare(
        "https://local.invalid/start?old=1",
        "",
        "get",
        "text/plain",
        "",
        &entries,
        &[0],
        encoding_rs::UTF_8,
    )
    .unwrap();
    assert_eq!(
        plan.url,
        "https://local.invalid/start?_charset_=UTF-8&_charset_=text"
    );
    for (action, method, enctype, target) in [
        ("file:///tmp/a", "get", "text/plain", ""),
        ("https://local.invalid/", "post", "multipart/form-data", ""),
        ("https://local.invalid/", "get", "text/plain", "_blank"),
    ] {
        assert!(form_navigation::prepare(
            "https://local.invalid/",
            action,
            method,
            enctype,
            target,
            &[],
            &[],
            encoding_rs::UTF_8
        )
        .is_err());
    }
}
