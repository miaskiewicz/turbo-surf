use turbo_surf_render::run_with_dom;

#[test]
fn idle_callback_timeout_has_zero_budget_after_registration_deadline() {
    let out = run_with_dom(
        "<body></body>",
        r#"(() => {
          let scheduled, result;
          const original = setTimeout;
          globalThis.setTimeout = callback => { scheduled = callback; return 1; };
          try {
            requestIdleCallback(deadline => {
              result = [deadline.didTimeout, deadline.timeRemaining() === 0];
            }, {timeout: 1});
            const end = performance.now() + 5;
            while (performance.now() < end) {}
            scheduled();
            return JSON.stringify(result);
          } finally { globalThis.setTimeout = original; }
        })()"#,
    )
    .unwrap();
    assert_eq!(out, "[true,true]");
}
