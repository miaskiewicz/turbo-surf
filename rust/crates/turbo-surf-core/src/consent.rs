//! Consent-wall bypass. Some sites (google, youtube, EU-region CMPs) serve a
//! JS-gated "before you continue" interstitial to a fresh visitor: the real page
//! markup is present but `display:none` until the user clicks *Accept*, which sets
//! a consent cookie and reloads. A headless engine can't run that click's JS, so
//! the page renders blank. Pre-seeding the cookie the interstitial *would* have set
//! makes the site return its real, server-rendered (JS-free) content directly.
//!
//! Opt-in via [`crate::net::FetchOptions::bypass_consent`]. The values below are the
//! minimal "dismiss the interstitial" cookies scrapers use; they carry no personal
//! data and are stable enough to hard-code (google rotates the opaque tail but
//! accepts this minimal `SOCS`).

/// Consent cookies (`name`, `value`) to send for `host`, or empty if the host has
/// no known consent wall. Matched on a host substring so country TLDs
/// (`google.co.uk`, `google.de`) and subdomains are covered.
pub fn cookies_for_host(host: &str) -> &'static [(&'static str, &'static str)] {
    let h = host.to_ascii_lowercase();
    // `SOCS` dismisses google's "Before you continue to Google" interstitial so the
    // real homepage / SERP (search box, results, footer) is served un-hidden.
    if h.contains("google.") || h.contains("youtube.") {
        return &[("SOCS", "CAESHAgBEhIaAB")];
    }
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_hosts_get_socs() {
        assert_eq!(
            cookies_for_host("www.google.com"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
        assert_eq!(
            cookies_for_host("google.co.uk"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
        assert_eq!(
            cookies_for_host("www.youtube.com"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
    }

    #[test]
    fn unknown_hosts_get_nothing() {
        assert!(cookies_for_host("example.com").is_empty());
        assert!(cookies_for_host("nike.com").is_empty());
    }
}
