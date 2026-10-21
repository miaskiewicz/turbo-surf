use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use turbo_surf_core::{
    cookies::CookieJar,
    net::{self, FetchOptions},
    request_context::ClientHints,
};
use turbo_surf_render::PageSession;

struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn fixture() -> (Fixture, Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../harness/detection/compression-dictionary-server.mjs")
        .canonicalize()
        .unwrap();
    let mut fixture = Fixture(
        Command::new("node")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(fixture.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(
        !line.is_empty(),
        "dictionary HTTPS fixture must announce its endpoints"
    );
    let urls = serde_json::from_str(&line).unwrap();
    (fixture, urls)
}
fn hints() -> ClientHints {
    ClientHints::with_client_builder(|| {
        net::client_builder_with_trust(include_bytes!("csp_report_cert.der"))
    })
}

fn lifecycle_projection(events: &Value, omitted: &str) -> Vec<(Value, Value)> {
    events
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] != omitted)
        .map(|event| (event[0].clone(), event[2].clone()))
        .collect()
}

fn lifecycle_events_match(actual: &Value, expected: &Value) -> bool {
    // Resource errors and Window timers use independent task sources. Each
    // projection keeps every other event, its count, lifecycle state and order.
    ["link:error", "future-timer"].iter().all(|omitted| {
        lifecycle_projection(actual, omitted) == lifecycle_projection(expected, omitted)
    })
}

#[test]
fn dictionary_lifecycle_order_keeps_barriers_counts_and_states() {
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/compression-dictionary-chrome155.json"
    ))
    .unwrap();
    for (id, case) in control["cases"].as_array().unwrap().iter().enumerate() {
        if !case["lifecycleDelay"].is_string() {
            continue;
        }
        let expected = &control["result"][id]["events"];
        let index = |name: &str| {
            expected
                .as_array()
                .unwrap()
                .iter()
                .position(|event| event[0] == name)
                .unwrap()
        };
        let error = index("link:error");
        let future = index("future-timer");
        assert_eq!(
            error.abs_diff(future),
            1,
            "independent pair must remain adjacent"
        );
        assert!(lifecycle_events_match(expected, expected));
        let mut reversed = expected.clone();
        reversed.as_array_mut().unwrap().swap(error, future);
        assert!(
            lifecycle_events_match(&reversed, expected),
            "{}",
            case["name"]
        );

        let mut missing = expected.clone();
        missing.as_array_mut().unwrap().remove(error);
        assert!(
            !lifecycle_events_match(&missing, expected),
            "missing error: {}",
            case["name"]
        );
        let mut duplicated = expected.clone();
        duplicated
            .as_array_mut()
            .unwrap()
            .insert(error, expected[error].clone());
        assert!(
            !lifecycle_events_match(&duplicated, expected),
            "duplicate error: {}",
            case["name"]
        );
        let mut duplicate_timer = expected.clone();
        duplicate_timer
            .as_array_mut()
            .unwrap()
            .insert(future, expected[future].clone());
        assert!(
            !lifecycle_events_match(&duplicate_timer, expected),
            "duplicate timer: {}",
            case["name"]
        );
        let mut timer_order = expected.clone();
        timer_order
            .as_array_mut()
            .unwrap()
            .swap(index("timer"), future);
        assert!(
            !lifecycle_events_match(&timer_order, expected),
            "timer order: {}",
            case["name"]
        );
        let mut load_barrier = expected.clone();
        load_barrier
            .as_array_mut()
            .unwrap()
            .swap(index("load"), index("DOMContentLoaded"));
        assert!(
            !lifecycle_events_match(&load_barrier, expected),
            "load barrier: {}",
            case["name"]
        );
        if case["lifecycleDelay"] != "image" {
            let mut script_barrier = expected.clone();
            script_barrier
                .as_array_mut()
                .unwrap()
                .swap(index("script"), index("DOMContentLoaded"));
            assert!(
                !lifecycle_events_match(&script_barrier, expected),
                "script barrier: {}",
                case["name"]
            );
        }
        for event in [error, future] {
            let mut state = expected.clone();
            state[event][2] = json!("complete");
            assert!(
                !lifecycle_events_match(&state, expected),
                "event state: {}",
                case["name"]
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dictionary_discovery_policies_and_nonblocking_lifecycle_match_chrome() {
    let (_fixture, urls) = fixture();
    let primary = urls["primary"].as_str().unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/compression-dictionary-chrome155.json"
    ))
    .unwrap();
    for (id, case) in control["cases"].as_array().unwrap().iter().enumerate() {
        let source = format!("{primary}/page?case={id}");
        let hints = hints();
        let response = net::fetch_html(
            &source,
            FetchOptions {
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut jar = CookieJar::new();
        let response_cookies = response
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        jar.set_from_response(&source, &response_cookies, 0.0);
        let cookie_state = jar.storage_state();
        let mut page = PageSession::open_with_response_headers(
            &response.html,
            &source,
            &cookie_state,
            "",
            20000,
            "",
            hints.clone(),
            &response.headers,
        )
        .await
        .unwrap();
        let resource_path = case["resource"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("/resource-{id}"));
        let target = format!(
            "{}{resource_path}?case={id}",
            if case["cross"] == true {
                urls["peer"].as_str().unwrap()
            } else {
                primary
            }
        );
        if case["head"] == true {
            let dictionary = format!("{primary}/dictionary?case={id}");
            let script=format!("(async()=>{{const first=await fetch({},{{method:'HEAD'}});const headBytes=(await first.text()).length;await fetch({}).then(r=>r.text());const second=await fetch({});const getBytes=(await second.text()).length;await fetch({}).then(r=>r.text());await new Promise(resolve=>setTimeout(resolve,100));await fetch({}).then(r=>r.text());await new Promise(resolve=>setTimeout(resolve,400));await fetch({}).then(r=>r.text());return {{headBytes,getBytes}}}})()",serde_json::to_string(&dictionary).unwrap(),serde_json::to_string(&(target.clone()+"&phase=before")).unwrap(),serde_json::to_string(&dictionary).unwrap(),serde_json::to_string(&(target.clone()+"&phase=after")).unwrap(),serde_json::to_string(&(target.clone()+"&phase=after100")).unwrap(),serde_json::to_string(&(target.clone()+"&phase=after500")).unwrap());
            let actual: Value =
                serde_json::from_str(&page.eval_json_expression(&script).await.unwrap()).unwrap();
            assert_eq!(actual, control["result"][id]["head"]);
        } else if case["prioritySpoof"] == true {
            let script=format!("fetch({},{{headers:{{Priority:'u=4, i'}},dictionary_prefetch:true}}).then(r=>({{ok:r.ok}})).catch(error=>({{error:error.name,message:error.message}}))",serde_json::to_string(&target).unwrap());
            let actual: Value =
                serde_json::from_str(&page.eval_json_expression(&script).await.unwrap()).unwrap();
            assert_eq!(actual, control["result"][id]["priority"]);
        } else if let Some(kind) = case["manual"].as_str() {
            let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(format!(
                "authored reusable compression dictionary case {id}"
            )));
            let supplied = if kind == "id" {
                json!({"Dictionary-ID":"\"forged\""})
            } else {
                json!({"Available-Dictionary":match kind {"matching"=>format!(":{hash}:"),"mismatch"=>":AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:".into(),_=>"malformed".into()}})
            };
            let script=format!("(async()=>{{const supplied={supplied},headers=new Headers(supplied),request=new Request({},{{headers}});const retained={{headers:[headers.get('available-dictionary'),headers.get('dictionary-id')],request:[request.headers.get('available-dictionary'),request.headers.get('dictionary-id')]}};try{{const response=await fetch(request);await response.text();return {{...retained,outcome:{{ok:response.ok}}}};}}catch(error){{return {{...retained,outcome:{{error:error.name,message:error.message}}}}}}}})()",serde_json::to_string(&target).unwrap());
            let actual: Value =
                serde_json::from_str(&page.eval_json_expression(&script).await.unwrap()).unwrap();
            assert_eq!(
                actual, control["result"][id]["manual"],
                "manual header admission: {}",
                case["name"]
            );
        } else {
            page.eval_json_expression(&format!(
                "fetch({},{{cache:{}}}).then(r=>r.text()).catch(error=>error.name)",
                serde_json::to_string(&target).unwrap(),
                serde_json::to_string(case["cache"].as_str().unwrap_or("default")).unwrap()
            ))
            .await
            .unwrap();
        }
        let events: Value =
            serde_json::from_str(&page.eval_json_expression("dictionaryEvents").await.unwrap())
                .unwrap();
        let wire = net::fetch_html(
            &format!("{primary}/wire"),
            FetchOptions {
                client_hints: Some(&hints),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let wire: Value = serde_json::from_str(&wire.html).unwrap();
        let actual: Vec<_> = wire
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["case"] == id && row["path"] == "/dictionary")
            .collect();
        let expected: Vec<_> = control["wire"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["case"] == id && row["path"] == "/dictionary")
            .collect();
        assert_eq!(
            actual.len(),
            expected.len(),
            "dictionary requests: {}",
            case["name"]
        );
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            assert_eq!(
                actual["method"], expected["method"],
                "dictionary discovery must not invent a preflight"
            );
            for field in [
                "sec-fetch-site",
                "sec-fetch-mode",
                "sec-fetch-dest",
                "accept",
            ] {
                assert_eq!(
                    actual["headers"][field], expected["headers"][field],
                    "{} {field}",
                    case["name"]
                );
            }
            for field in ["cookie", "referer"] {
                if case["head"] == true {
                    assert_eq!(
                        actual["headers"][field].is_string(),
                        expected["headers"][field].is_string(),
                        "ordinary fetch retains its normal {field} policy"
                    );
                    continue;
                }
                assert!(
                    actual["headers"].get(field).is_none(),
                    "dictionary must omit {field}"
                );
            }
            assert_eq!(
                actual["headers"].get("origin").is_some(),
                expected["headers"].get("origin").is_some()
            );
            assert_eq!(
                actual["aborted"], expected["aborted"],
                "dictionary cancellation: {}",
                case["name"]
            );
            if expected["aborted"] == true {
                // Chrome collection waits beyond the fixture's body timer; native
                // cancellation can finish hydration before that timer fires.
                continue;
            }
            let finished = actual["finished"]
                .as_u64()
                .unwrap_or_else(|| panic!("dictionary body incomplete: {}", case["name"]));
            for event in events
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event[0] == "load" || event[0] == "DOMContentLoaded")
            {
                assert!(
                    event[1].as_u64().unwrap() < finished,
                    "dictionary must not block {}",
                    event[0]
                );
            }
        }
        if case["lifecycleDelay"].is_string() {
            assert!(
                lifecycle_events_match(&events, &control["result"][id]["events"]),
                "resource task lifecycle events: {}: actual {events}, expected {}",
                case["name"],
                control["result"][id]["events"]
            );
        } else {
            let names = |rows: &Value| {
                rows.as_array()
                    .unwrap()
                    .iter()
                    .map(|event| event[0].clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                names(&events),
                names(&control["result"][id]["events"]),
                "events: {}",
                case["name"]
            );
        }

        if case["prioritySpoof"] == true {
            let requests = |rows: &Value| {
                rows.as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["case"] == id && row["path"] == resource_path)
                    .map(|row| row["method"].clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                requests(&wire),
                requests(&control["wire"]),
                "guest Priority must retain actual preflight, regardless of spoof marker"
            );
        }
        let available = |rows: &Value| {
            rows.as_array()
                .unwrap()
                .iter()
                .filter(|row| row["case"] == id && row["path"] == resource_path)
                .map(|row| row["headers"].get("available-dictionary").is_some())
                .collect::<Vec<_>>()
        };
        let mut actual_available = available(&wire);
        let mut expected_available = available(&control["wire"]);
        if case["head"] == true {
            assert_eq!(actual_available.len(), 4);
            assert_eq!(expected_available.len(), 4);
            // Chrome publishes its dictionary asynchronously after response.text();
            // Core publishes at EOF. Compare HEAD absence and stable delayed reads,
            // retaining the immediate Chrome observation in the source-pinned fixture.
            actual_available.remove(1);
            expected_available.remove(1);
        }
        assert_eq!(
            actual_available, expected_available,
            "dictionary negotiation: {}",
            case["name"]
        );
    }
}
