//! Count actual native layout builds, independently of elapsed time or guest globals.
use std::ffi::OsString;
use std::path::PathBuf;
use turbo_surf_render::PageSession;

static PROFILE_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct ProfileDirectory {
    path: PathBuf,
    trace: Option<OsString>,
    profile: Option<OsString>,
}

impl ProfileDirectory {
    fn new(name: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "turbo-surf-query-cache-{}-{name}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        let trace = std::env::var_os("TURBO_SURF_JS_TRACE");
        let profile = std::env::var_os("TURBO_SURF_JS_PROFILE");
        std::env::set_var("TURBO_SURF_JS_TRACE", &path);
        std::env::set_var("TURBO_SURF_JS_PROFILE", "1");
        Self {
            path,
            trace,
            profile,
        }
    }

    fn read(&self) -> serde_json::Value {
        let isolate = std::fs::read_dir(&self.path)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        serde_json::from_slice(&std::fs::read(isolate.join("layout-profile.json")).unwrap())
            .unwrap()
    }
}

impl Drop for ProfileDirectory {
    fn drop(&mut self) {
        for (name, value) in [
            ("TURBO_SURF_JS_TRACE", &self.trace),
            ("TURBO_SURF_JS_PROFILE", &self.profile),
        ] {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}

async fn profiled_page(
    name: &str,
    html: &str,
) -> (PageSession, serde_json::Value, ProfileDirectory) {
    let directory = ProfileDirectory::new(name);
    let page = PageSession::open(html, "https://query-cache.test/", "", "", 5000)
        .await
        .unwrap();
    assert!(!page.hydration_diagnostics().budget_exceeded);
    let profile = directory.read();
    (page, profile, directory)
}

#[tokio::test(flavor = "current_thread")]
async fn many_distinct_static_targets_share_one_strictly_admitted_layout() {
    let _environment = PROFILE_ENV.lock().await;
    let mut html = String::from(
        "<!doctype html><style>input{box-sizing:border-box;width:40px;height:20px}</style><body>",
    );
    for index in 0..12 {
        html.push_str(&format!("<input id=target-{index}>"));
    }
    html.push_str(
        "<script>globalThis.widths=Array.from(document.querySelectorAll('input'),element=>element.getBoundingClientRect().width)</script>",
    );
    let (mut page, profile, _directory) = profiled_page("distinct", &html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        serde_json::to_string(&vec![40; 12]).unwrap()
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 1);
    assert_eq!(profile["cacheMisses"], 1);
    assert_eq!(profile["cacheHits"], 11);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn mutations_and_cssom_changes_invalidate_the_shared_strict_layout() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>input{box-sizing:border-box;width:40px;height:20px}</style>
      <body><input id=a><input id=b><script>
      const read=()=>[a,b].map(element=>element.getBoundingClientRect().width);
      globalThis.widths=[read()];
      a.style.width='70px';widths.push(read());
      document.styleSheets[0].cssRules[0].style.width='90px';widths.push(read());
      </script>"#;
    let (mut page, profile, _directory) = profiled_page("mutations", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[[40,40],[70,40],[70,90]]"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 3);
    assert_eq!(profile["cacheMisses"], 3);
    assert_eq!(profile["cacheHits"], 3);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn query_relaxed_animation_admission_is_not_shared_with_other_reads() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>
      @keyframes spin{to{transform:rotate(360deg)}}
      #clip{position:absolute;left:500px;top:500px;width:40px;height:40px;overflow:hidden}
      #spin{width:20px;height:20px;animation:spin 1s linear infinite}
      #target{width:60px;height:20px}
      </style><body><div id=clip><div id=spin></div></div><div id=target></div>
      <script>
      globalThis.answers=[target.getBoundingClientRect().width];
      for(const read of [()=>spin.getBoundingClientRect().width,()=>document.body.scrollHeight]){
        try{answers.push(read())}catch(error){answers.push(error.message.includes('animation resource'))}
      }
      </script>"#;
    let (mut page, profile, _directory) = profiled_page("query-policy", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(answers)")
            .await
            .unwrap(),
        "[60,true,true]"
    );
    assert_eq!(profile["cacheHits"], 0);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn activating_an_animation_invalidates_a_previous_strict_certificate() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>
      @keyframes spin{to{transform:rotate(360deg)}}
      #clip{position:absolute;left:500px;top:500px;width:40px;height:40px;overflow:hidden}
      #spin{width:20px;height:20px}
      #target{width:60px;height:20px}
      </style><body><div id=clip><div id=spin></div></div><div id=target></div>
      <script>
      globalThis.answers=[target.getBoundingClientRect().width];
      const sheet=document.styleSheets[0];
      sheet.insertRule('#spin{animation:spin 1s linear infinite}',sheet.cssRules.length);
      answers.push(target.getBoundingClientRect().width);
      for(const read of [()=>spin.getBoundingClientRect().width,()=>document.body.scrollHeight]){
        try{answers.push(read())}catch(error){answers.push(error.message.includes('animation resource'))}
      }
      </script>"#;
    let (mut page, profile, _directory) = profiled_page("activation", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(answers)")
            .await
            .unwrap(),
        "[60,60,true,true]"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 4);
    assert_eq!(profile["cacheHits"], 0);
    page.close();
}
