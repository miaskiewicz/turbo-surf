//! End-to-end reCAPTCHA solver test — the core [`RecaptchaSolver`] driven by the
//! render tier's [`V8RecaptchaEngine`], all offline against a localhost fixture that
//! stands in for google's reCAPTCHA endpoints (page + `api.js` main VM + bframe
//! document + bframe VM). Its own test binary so it doesn't share a V8-platform init
//! with the vendored binding's standalone-V8 unit test (see `render.rs`).

use turbo_surf_core::challenge::{Challenge, ChallengeSolver, SolveContext, SolveError, Vendor};
use turbo_surf_core::recaptcha::RecaptchaSolver;
use turbo_surf_render::V8RecaptchaEngine;

// The solver runs the page's OWN reCAPTCHA integration in the isolate: api.js installs
// `grecaptcha`, `execute()` opens the bframe iframe (bridged to a real second realm +
// fetched over the net stack), the parent↔bframe postMessage handshake yields a client
// token, and the solver returns it as `g-recaptcha-response`.
#[tokio::test]
async fn solver_mints_a_client_token_end_to_end() {
    let port = spawn_recaptcha_site().await;
    let base = format!("http://127.0.0.1:{port}");

    let solver = RecaptchaSolver::new().with_engine(Box::new(V8RecaptchaEngine));
    // sitekey left None on purpose — the solver fetches the page and extracts it from
    // the `render=` param (exercises detection + extraction end to end).
    let ch = Challenge::new(Vendor::Recaptcha, format!("{base}/"));

    let token = solver
        .solve(&ch, &SolveContext::default())
        .await
        .expect("the in-isolate flow must mint a client token");
    let resp = token
        .headers
        .iter()
        .find(|(k, _)| k == "g-recaptcha-response")
        .map(|(_, v)| v.clone())
        .expect("token surfaced under g-recaptcha-response");
    assert_eq!(
        resp, "bframe-token-SITEKEY-XYZ",
        "token must echo the sitekey the parent posted into the bframe realm, got {resp:?}"
    );
    assert!(
        token.cookies.is_empty(),
        "reCAPTCHA token is a form field, not a cookie"
    );
}

// A v2 image-grid checkbox page (no `render=`, not invisible) is refused with the
// documented error — the honest boundary — without even consulting the engine.
#[tokio::test]
async fn solver_refuses_v2_image_checkbox_end_to_end() {
    let port = spawn_v2_checkbox_site().await;
    let solver = RecaptchaSolver::new().with_engine(Box::new(V8RecaptchaEngine));
    let ch = Challenge::new(Vendor::Recaptcha, format!("http://127.0.0.1:{port}/"));
    let err = solver.solve(&ch, &SolveContext::default()).await;
    assert!(
        matches!(err, Err(SolveError::VisualChallenge)),
        "v2 image checkbox must return VisualChallenge, got {err:?}"
    );
}

// --- fixtures ---------------------------------------------------------------

// A minimal reCAPTCHA v3 site: the page loads api.js (the stand-in main VM), whose
// execute() opens the bframe iframe and drives the postMessage handshake. Routes by
// path, matching google's real endpoint shapes closely enough for the render tier's
// BFRAME_RE + handshake to fire.
async fn spawn_recaptcha_site() -> u16 {
    let page = r#"<!DOCTYPE html><html><head>
<script src="/recaptcha/api.js?render=SITEKEY-XYZ"></script>
</head><body>
<form>
  <div class="g-recaptcha" data-sitekey="SITEKEY-XYZ" data-size="invisible"></div>
  <textarea id="g-recaptcha-response" name="g-recaptcha-response"></textarea>
</form>
</body></html>"#;
    // The main VM: install grecaptcha; execute() runs the bframe handshake and resolves
    // the token the bframe VM posts back.
    let api_js = r#"
(function () {
  window.grecaptcha = {
    ready: function (cb) { cb(); },
    render: function () { return 0; },
    execute: function (sitekey, opts) {
      return new Promise(function (resolve) {
        var iframe = document.createElement('iframe');
        window.addEventListener('message', function (e) {
          var m = e.data || {};
          if (m.type === 'bframe-ready' && e.source === iframe.contentWindow) {
            iframe.contentWindow.postMessage({ type: 'challenge', c: sitekey }, '*');
          } else if (m.type === 'token' && e.source === iframe.contentWindow) {
            resolve(m.token);
          }
        });
        iframe.src = location.origin + '/recaptcha/api2/bframe?k=' + sitekey;
        document.body.appendChild(iframe);
      });
    }
  };
})();
"#;
    let bframe_doc = r#"<!DOCTYPE html><html><head><script src="/recaptcha/api2/frame-vm.js"></script></head><body></body></html>"#;
    // The bframe VM (runs in the child realm): greet the parent on load, answer the
    // challenge with a token derived from the sitekey, posted back through e.source.
    let frame_vm = r#"
addEventListener('message', function (e) {
  var m = e.data || {};
  if (m.type === 'challenge') {
    e.source.postMessage({ type: 'token', token: 'bframe-token-' + m.c }, '*');
  }
});
addEventListener('load', function () {
  parent.postMessage({ type: 'bframe-ready' }, '*');
});
"#;
    spawn_router(page, api_js, bframe_doc, frame_vm).await
}

// A v2 checkbox site: a plain widget (no render=, not invisible) → the image-grid case.
async fn spawn_v2_checkbox_site() -> u16 {
    let page = r#"<!DOCTYPE html><html><head>
<script src="/recaptcha/api.js"></script>
</head><body>
<form><div class="g-recaptcha" data-sitekey="SITEKEY-CHECKBOX"></div></form>
</body></html>"#;
    spawn_router(page, "", "", "").await
}

async fn spawn_router(
    page: &'static str,
    api_js: &'static str,
    bframe_doc: &'static str,
    frame_vm: &'static str,
) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]).to_string();
            let path = req
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .split('?')
                .next()
                .unwrap_or("/")
                .to_string();
            // frame-vm.js must be checked before api.js (both end in `.js`).
            let (ctype, body): (&str, &str) = if path.contains("frame-vm.js") {
                ("application/javascript", frame_vm)
            } else if path.contains("api.js") {
                ("application/javascript", api_js)
            } else if path.contains("bframe") {
                ("text/html", bframe_doc)
            } else {
                ("text/html", page)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.flush().await;
        }
    });
    port
}
