//! `TURBO_SURF_TZ` is the operator's explicit time-zone opt-in: set before the V8
//! platform starts, page `Date` uses that zone. Its own binary (one process, one test)
//! so no other test has initialized the platform first.

#[test]
fn turbo_surf_tz_pins_the_page_time_zone() {
    // Asia/Kolkata has no DST: UTC+05:30 at every instant.
    std::env::set_var("TURBO_SURF_TZ", "Asia/Kolkata");
    let result = turbo_surf_render::run_with_dom(
        "<body>",
        "[new Date(0).getTimezoneOffset(),new Date(Date.UTC(2026,6,1)).getTimezoneOffset(),['Asia/Kolkata','Asia/Calcutta'].includes(Intl.DateTimeFormat().resolvedOptions().timeZone)].join()",
    )
    .unwrap();
    // ICU reports the zone under either of its two canonical names.
    assert_eq!(result, "-330,-330,true");
}
