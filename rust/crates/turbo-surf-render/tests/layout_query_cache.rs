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
    let started = std::time::Instant::now();
    let opened = PageSession::open(html, "https://query-cache.test/", "", "", 5000).await;
    let failed = match &opened {
        Ok(page) => page.hydration_diagnostics().budget_exceeded,
        Err(_) => true,
    };
    if failed {
        match &opened {
            Ok(page) => eprintln!(
                "LAYOUT_QUERY_CACHE_DIAG name={name} open_result=retained_ok elapsed_open_ms={} hydration={:?}",
                started.elapsed().as_millis(),
                page.hydration_diagnostics()
            ),
            Err(error) => eprintln!(
                "LAYOUT_QUERY_CACHE_DIAG name={name} open_result=error elapsed_open_ms={} error={error:?}",
                started.elapsed().as_millis()
            ),
        }
        let profile = std::fs::read_dir(&directory.path).ok().and_then(|entries| {
            entries.filter_map(Result::ok).find_map(|entry| {
                std::fs::read_to_string(entry.path().join("layout-profile.json")).ok()
            })
        });
        eprintln!("LAYOUT_QUERY_CACHE_DIAG name={name} native_profile={profile:?}");
    }
    let page = opened.unwrap();
    assert!(!page.hydration_diagnostics().budget_exceeded);
    let profile = directory.read();
    (page, profile, directory)
}

#[tokio::test(flavor = "current_thread")]
async fn detached_render_construction_reuses_connected_document_geometry() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>
      #target{width:60px;height:20px;padding:0;border:0}
      </style><body><div id=target></div><script>
      const target=document.getElementById('target'),scratch=[];
      globalThis.widths=[];
      for(let index=0;index<6;index++){
        widths.push(target.clientWidth);
        const host=document.createElement('div');
        const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
        icon.appendChild(document.createElementNS('http://www.w3.org/2000/svg','path'));
        host.appendChild(icon);host.setAttribute('class','detached');
        host.style.padding='9px';host.textContent='detached text';
        const input=document.createElement('input');input.defaultChecked=false;
        host.appendChild(input);scratch.push(host);
      }
      globalThis.allDetached=scratch.every(node=>!node.isConnected);
      </script>"#;
    let (mut page, profile, _directory) = profiled_page("detached-render", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[60,60,60,60,60,60]"
    );
    assert_eq!(
        page.eval_expression("String(allDetached)").await.unwrap(),
        "true"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 1);
    assert_eq!(profile["cacheMisses"], 1);
    assert_eq!(profile["cacheHits"], 5);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn connected_shadow_trees_preserve_detached_construction_cache_reuse() {
    let _environment = PROFILE_ENV.lock().await;
    for mode in ["open", "closed"] {
        let html = format!(
            r#"<!doctype html><div id="target" style="width:60px;height:20px"></div>
<next-route-announcer id="host"></next-route-announcer><script>
const shadow=document.querySelector('#host').attachShadow({{mode:'{mode}'}});
shadow.innerHTML='<span>Announcement</span>';
globalThis.widths=[];globalThis.scratch=[];
for(let index=0;index<6;index++){{
  widths.push(document.querySelector('#target').clientWidth);
  const node=document.createElement('div');node.textContent='detached';scratch.push(node);
}}
</script>"#
        );
        let (mut page, profile, _directory) = profiled_page("connected-shadow", &html).await;
        assert_eq!(
            page.eval_expression("JSON.stringify(widths)")
                .await
                .unwrap(),
            "[60,60,60,60,60,60]",
            "mode={mode}"
        );
        assert_eq!(profile["stages"]["layout"]["calls"], 1, "mode={mode}");
        assert_eq!(profile["cacheMisses"], 1, "mode={mode}");
        assert_eq!(profile["cacheHits"], 5, "mode={mode}");
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn detached_allocations_reuse_prepared_sheet_targets() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>#target{width:60px;height:20px}</style>
<div id="target"></div><script>
globalThis.widths=[];globalThis.scratch=[];
for(let index=0;index<6;index++){
  widths.push(document.querySelector('#target').clientWidth);
  scratch.push(document.createElement('div'));
}
</script>"#;
    let (mut page, profile, _directory) = profiled_page("detached-preparation", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[60,60,60,60,60,60]"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 1);
    assert_eq!(profile["cacheHits"], 5);
    assert_eq!(profile["stages"]["sheetExport"]["calls"], 1);
    assert_eq!(profile["stages"]["selectorPreparation"]["calls"], 1);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_targets_refresh_for_checked_state_and_cssom() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>
#target{width:20px;height:20px}
#host:has(input:checked) #target{width:40px}
#host:has(input:focus) #target{width:80px!important}
</style><div id="host"><input id="input" type="checkbox"><div id="target"></div></div><script>
const target=document.querySelector('#target'),input=document.querySelector('#input');
globalThis.widths=[target.clientWidth];
globalThis.scratch=document.createElement('div');widths.push(target.clientWidth);
input.checked=true;widths.push(target.clientWidth);widths.push(target.clientWidth);
document.styleSheets[0].insertRule('#host:has(input:checked) #target{width:60px}',3);
widths.push(target.clientWidth);
</script>"#;
    let (mut page, profile, _directory) = profiled_page("prepared-selector-state", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[20,20,40,40,60]"
    );
    assert_eq!(profile["stages"]["selectorPreparation"]["calls"], 3);
    page.close();
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
    assert_eq!(profile["stages"]["sheetExport"]["calls"], 1);
    assert_eq!(profile["stages"]["selectorPreparation"]["calls"], 1);
    assert_eq!(
        profile["stages"]["animationInspection"]["calls"]
            .as_u64()
            .unwrap_or(0),
        0,
        "original-safe native geometry must not enter canonical lifecycle inspection"
    );
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
    assert_eq!(
        profile["stages"]["animationInspection"]["calls"]
            .as_u64()
            .unwrap_or(0),
        0
    );
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

#[tokio::test(flavor = "current_thread")]
async fn geometry_safe_constant_effects_need_no_lifecycle_permission() {
    let _environment = PROFILE_ENV.lock().await;
    let mut html = String::from("<!doctype html><style>@keyframes pulse{from{opacity:.2}to{opacity:1}}input{box-sizing:border-box;width:40px;height:20px;animation:pulse 1s infinite}</style><body>");
    for index in 0..12 {
        html.push_str(&format!("<input id=target-{index}>"));
    }
    html.push_str("<script>globalThis.widths=Array.from(document.querySelectorAll('input'),element=>element.getBoundingClientRect().width)</script>");
    let (mut page, profile, _directory) = profiled_page("constant-effect", &html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        serde_json::to_string(&vec![40; 12]).unwrap()
    );
    assert_eq!(
        profile["stages"]["animationInspection"]["calls"]
            .as_u64()
            .unwrap_or(0),
        0,
        "a native geometry-safe animation must keep the original no-probe path"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 1);
    assert_eq!(profile["cacheHits"], 11);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn timer_only_post_load_work_hands_off_without_losing_finite_document_work() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _environment = PROFILE_ENV.lock().await;
    let _directory = ProfileDirectory::new("finite-work-handoff");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert!(count > 0, "actual fetch headers must complete");
            request.extend_from_slice(&bytes[..count]);
            assert!(request.len() <= 16 * 1024);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\npayload")
            .await
            .unwrap();
        String::from_utf8(request).unwrap()
    });
    let html = r#"<!doctype html><style>
      @keyframes spin{to{transform:rotate(360deg)}}
      #clip{position:absolute;left:500px;top:500px;width:40px;height:40px;overflow:hidden}
      #spin{width:20px;height:20px;animation:spin 1s linear infinite}
      #target{width:60px;height:20px}
      </style><body id=host><div id=clip><div id=spin></div></div><div id=target></div><div id=finite></div>
      <script>
      const originalDocument=document;
      target.getBoundingClientRect();
      try{spin.getBoundingClientRect()}catch(error){document.body.dataset.guarded=String(error.message.includes('animation resource'))}
      addEventListener('load',()=>{
        document.body.dataset.load='done';
        requestAnimationFrame(()=>{document.body.dataset.raf='done'});
        document.getElementById('finite').animate({opacity:[0,1]},64).finished.then(()=>{
          document.body.dataset.finished='done';
        });
        fetch('/payload').then(response=>response.text()).then(body=>{
          document.body.dataset.fetched=body;
        });
        setTimeout(()=>{
          document.body.dataset.delayed='done';
          document.body.dataset.sameDocument=String(document===originalDocument);
          Promise.resolve().then(()=>{document.body.dataset.microtask='done'});
          const script=document.createElement('script');
          script.textContent="document.body.dataset.dynamic='done';Promise.resolve().then(()=>{document.body.dataset.dynamicMicrotask='done'})";
          document.body.append(script);
        },50);
      });
      </script>"#;
    let started = std::time::Instant::now();
    let mut page = PageSession::open(html, &format!("{origin}/page"), "", "", 5000)
        .await
        .unwrap();
    // Serialization reads the retained native document, without another JS evaluation or pump.
    let snapshot = page.serialize_checked().unwrap();
    let diagnostics = page.hydration_diagnostics().clone();
    eprintln!(
        "LAYOUT_HANDOFF_CONTROL elapsed_open_ms={} hydration={diagnostics:?}",
        started.elapsed().as_millis()
    );
    let tree = turbo_dom_parser::rtdom::Tree::parse(&snapshot);
    let body = tree.get_element_by_id("host").unwrap();
    for name in [
        "data-load",
        "data-raf",
        "data-finished",
        "data-delayed",
        "data-microtask",
        "data-dynamic",
        "data-dynamic-microtask",
    ] {
        assert_eq!(
            tree.get_attribute(body, name),
            Some("done"),
            "{name}: {snapshot}"
        );
    }
    assert_eq!(tree.get_attribute(body, "data-fetched"), Some("payload"));
    assert_eq!(tree.get_attribute(body, "data-same-document"), Some("true"));
    assert_eq!(tree.get_attribute(body, "data-guarded"), Some("true"));
    let request = server.await.unwrap();
    assert!(
        request.starts_with("GET /payload HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(!diagnostics.budget_exceeded, "{diagnostics:?}");
    assert!(
        diagnostics.progress.completed_pumps < 500,
        "timer-only work exhausted the native-work pump instead of reaching full quiescence: {diagnostics:?}"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_sheets_observe_cssom_dom_and_live_selector_changes() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style>input{box-sizing:border-box;padding:0;border:0;width:10px;height:20px}</style>
      <style id=rules>input:checked{width:30px}</style><body><input id=control type=checkbox><script>
      globalThis.widths=[];
      const read=()=>{widths.push(control.getBoundingClientRect().width);widths.push(control.getBoundingClientRect().width)};
      read();control.checked=true;read();
      rules.sheet.cssRules[0].style.width='50px';read();
      rules.textContent='input:checked{width:70px}';read();
      rules.sheet.disabled=true;read();rules.sheet.disabled=false;read();
      rules.sheet.media.mediaText='not all';read();rules.sheet.media.mediaText='all';read();
      const adopted=new CSSStyleSheet();adopted.replaceSync('input:checked{width:90px}');
      document.adoptedStyleSheets=[adopted];read();document.adoptedStyleSheets=[];read();
      </script>"#;
    let (mut page, profile, _directory) = profiled_page("sheet-invalidation", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[10,10,30,30,50,50,70,70,10,10,70,70,10,10,70,70,90,90,70,70]"
    );
    assert!(profile["stages"]["sheetExport"]["calls"].as_u64().unwrap() < 40);
    assert!(
        profile["stages"]["selectorPreparation"]["calls"]
            .as_u64()
            .unwrap()
            < 40
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn dom_mutations_reuse_raw_sheets_but_rematch_current_targets() {
    let _environment = PROFILE_ENV.lock().await;
    let html = "<!doctype html><style>#target{box-sizing:border-box;width:40px;height:20px}#target.ready{width:70px}</style><div id=target></div><script>const target=document.getElementById('target');globalThis.widths=Array.from({length:12},(_,index)=>{target.className=index%2?'ready':'';target.setAttribute('data-version',String(index));return target.getBoundingClientRect().width;});</script>";
    let (mut page, profile, _directory) = profiled_page("raw-sheets", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[40,70,40,70,40,70,40,70,40,70,40,70]"
    );
    assert_eq!(profile["stages"]["sheetExport"]["calls"], 1);
    assert_eq!(profile["stages"]["selectorPreparation"]["calls"], 12);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn raw_sheet_reuse_tracks_native_media_disabled_rules_and_sheet_identity() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style id=rules>#target{width:40px!important}</style><div id=target style="width:10px;height:20px"></div><script>
      const target=document.getElementById('target'),owner=document.getElementById('rules'),sheet=owner.sheet;
      const widths=[],read=()=>widths.push(target.getBoundingClientRect().width);
      read();owner.setAttribute('media','not all');read();owner.setAttribute('media','all');read();
      sheet.disabled=true;read();sheet.disabled=false;read();
      sheet.cssRules[0].style.setProperty('width','60px','important');read();
      owner.textContent='#target{width:70px!important}#target.ready{width:80px!important}';read();
      target.className='ready';read();target.className='';read();
      sheet.cssRules[0].style.setProperty('width','90px','important');read();
      globalThis.widths=widths;
    </script>"#;
    let (mut page, _profile, _directory) = profiled_page("raw-sheet-state", html).await;
    assert_eq!(
        page.eval_expression("JSON.stringify(widths)")
            .await
            .unwrap(),
        "[40,10,40,10,40,60,70,80,70,70]"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn warmed_own_none_rectangles_do_not_enter_layout_capture() {
    let _environment = PROFILE_ENV.lock().await;
    let html = r#"<!doctype html><style id=sheet>input{box-sizing:border-box;width:40px;height:20px}</style>
      <body><input id=visible><script>
      const subject=document.getElementById('sheet');
      globalThis.position=getComputedStyle(subject).position;
      globalThis.hiddenRects=Array.from({length:12},()=>{
        const rect=subject.getBoundingClientRect();
        return [rect.x,rect.y,rect.width,rect.height,subject.getClientRects().length];
      });
      globalThis.visibleWidth=document.getElementById('visible').getBoundingClientRect().width;
      </script>"#;
    let (mut page, profile, directory) = profiled_page("cached-own-none", html).await;
    assert_eq!(
        page.eval_json_expression("hiddenRects").await.unwrap(),
        serde_json::to_string(&vec![vec![0; 5]; 12]).unwrap()
    );
    assert_eq!(
        page.eval_expression("String(visibleWidth)").await.unwrap(),
        "40"
    );
    assert_eq!(profile["stages"]["layout"]["calls"], 1);
    assert_eq!(
        profile["cacheHits"], 0,
        "proven empty rect reads must return before capture and layout cache lookup"
    );
    page.close();
    drop(directory);
    let metric_html = html.replace(
        "globalThis.visibleWidth=",
        "globalThis.clientWidth=subject.clientWidth;globalThis.visibleWidth=",
    );
    let (mut page, metric_profile, _directory) =
        profiled_page("cached-own-none-metric", &metric_html).await;
    assert_eq!(
        page.eval_expression("String(globalThis.clientWidth)")
            .await
            .unwrap(),
        "0"
    );
    let calls = |value: &serde_json::Value| {
        value["cacheHits"].as_u64().unwrap_or(0) + value["cacheMisses"].as_u64().unwrap_or(0)
    };
    assert!(
        calls(&metric_profile) > calls(&profile),
        "client metrics must keep their normal native geometry path"
    );
    page.close();
}
