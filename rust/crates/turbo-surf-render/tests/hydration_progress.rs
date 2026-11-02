//! Native lifecycle evidence is independent of guest globals and survives a watchdog stop.
use turbo_surf_render::{HydrationPhase, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_phase_and_opt_in_cpu_profile_survive_runaway_microtasks() {
    let directory = std::env::temp_dir().join(format!(
        "turbo-surf-hydration-profile-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::env::set_var("TURBO_SURF_JS_TRACE", &directory);
    std::env::remove_var("TURBO_SURF_JS_PROFILE");
    // Warm process-wide font data outside the deliberately short watchdog run.
    let warm = PageSession::open(
        "<body><script>document.body.getBoundingClientRect()</script>",
        "https://hydration.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    warm.close();
    let isolate = std::fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!isolate.join("layout-profile.json").exists());
    assert!(!isolate.join("cpu-profile.json").exists());
    std::fs::remove_dir_all(&directory).unwrap();
    std::env::set_var("TURBO_SURF_JS_TRACE", &directory);
    std::env::set_var("TURBO_SURF_JS_PROFILE", "1");
    let opened = PageSession::open(
        "<body><script>document.body.getBoundingClientRect();function keepScheduling(){queueMicrotask(keepScheduling)};queueMicrotask(keepScheduling)</script>",
        "https://hydration.test/",
        "",
        "",
        150,
    )
    .await;
    std::env::remove_var("TURBO_SURF_JS_TRACE");
    std::env::remove_var("TURBO_SURF_JS_PROFILE");
    let mut page = opened.unwrap();
    let diagnostics = page.hydration_diagnostics();
    assert!(diagnostics.budget_exceeded, "{diagnostics:?}");
    assert_eq!(
        diagnostics.progress.phase,
        HydrationPhase::ParserAndDeferredScripts
    );
    assert_eq!(diagnostics.progress.completed_pumps, 0);
    assert_eq!(page.eval_expression("1 + 1").await.unwrap(), "2");
    page.close();
    let isolate = std::fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(isolate.join("cpu-profile.json")).unwrap()).unwrap();
    assert!(profile["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| { node["callFrame"]["functionName"] == "keepScheduling" }));
    assert!(!profile["samples"].as_array().unwrap().is_empty());
    let inventory: serde_json::Value =
        serde_json::from_slice(&std::fs::read(isolate.join("inventory.json")).unwrap()).unwrap();
    assert_eq!(inventory["cpuProfileSaved"], true);
    assert_eq!(inventory["cpuProfilePending"], false);
    assert_eq!(
        inventory["cpuProfileSetup"]["requestedIntervalMicroseconds"],
        1000
    );
    let commands = inventory["cpuProfileSetup"]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 3);
    assert!(commands
        .iter()
        .all(|command| command["acknowledged"] == true));
    assert_eq!(inventory["errors"], serde_json::json!([]));
    let layout: serde_json::Value =
        serde_json::from_slice(&std::fs::read(isolate.join("layout-profile.json")).unwrap())
            .unwrap();
    assert_eq!(layout["timingUnit"], "microseconds");
    assert_eq!(layout["cacheHits"], 0);
    assert_eq!(layout["cacheMisses"], 1);
    assert_eq!(layout["stages"]["capture"]["calls"], 1);
    assert_eq!(layout["stages"]["layout"]["calls"], 1);
    assert_eq!(
        layout["scope"],
        "Initial hydration only; host stages overlap inclusively."
    );
    let mut keys = layout
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["cacheHits", "cacheMisses", "scope", "stages", "timingUnit"]
    );
    std::fs::remove_dir_all(&directory).unwrap();
    std::env::set_var("TURBO_SURF_JS_TRACE", &directory);
    std::env::set_var("TURBO_SURF_JS_PROFILE", "1");
    let failed = PageSession::open_with_init_script(
        "<body>",
        "https://hydration.test/",
        "",
        "",
        5000,
        // The second read of the unchanged document is served from the layout cache.
        "document.body.getBoundingClientRect();document.body.getBoundingClientRect();throw Error('authored init failure')",
    )
    .await;
    std::env::remove_var("TURBO_SURF_JS_TRACE");
    std::env::remove_var("TURBO_SURF_JS_PROFILE");
    assert!(
        matches!(&failed, Err(error) if error.contains("authored init failure")),
        "{:?}",
        failed.as_ref().err()
    );
    let isolate = std::fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let inventory: serde_json::Value =
        serde_json::from_slice(&std::fs::read(isolate.join("inventory.json")).unwrap()).unwrap();
    assert_eq!(inventory["cpuProfileSaved"], true);
    assert_eq!(inventory["cpuProfilePending"], false);
    assert_eq!(inventory["errors"], serde_json::json!([]));
    let layout: serde_json::Value =
        serde_json::from_slice(&std::fs::read(isolate.join("layout-profile.json")).unwrap())
            .unwrap();
    assert_eq!(layout["stages"]["layout"]["calls"], 1);
    assert_eq!(layout["stages"]["rulesKey"]["calls"], 2);
    assert_eq!(layout["cacheMisses"], 1);
    assert_eq!(layout["cacheHits"], 1);
    std::fs::remove_dir_all(&directory).unwrap();
    std::env::set_var("TURBO_SURF_JS_TRACE", &directory);
    let page = PageSession::open(
        "<body><script>document.body.getBoundingClientRect()</script>",
        "https://hydration.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    std::env::remove_var("TURBO_SURF_JS_TRACE");
    assert!(!page.hydration_diagnostics().budget_exceeded);
    assert_eq!(
        page.hydration_diagnostics().progress.phase,
        HydrationPhase::Complete
    );
    page.close();
    let isolate = std::fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!isolate.join("layout-profile.json").exists());
    assert!(!isolate.join("cpu-profile.json").exists());
    std::fs::remove_dir_all(&directory).unwrap();
}
