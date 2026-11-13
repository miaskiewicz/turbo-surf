use serde::{Deserialize, Serialize};

/// Host-owned context preferences; cloned for each new tab and updated through
/// native request-state handles, never writable by guest page scripts.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserEmulation {
    pub(crate) color_scheme: Option<String>,
    pub(crate) reduced_motion: Option<String>,
    pub(crate) locale: Option<String>,
    pub(crate) geolocation: Option<crate::geolocation_config::Geolocation>,
    #[serde(default)]
    pub(crate) permissions: Vec<crate::geolocation_config::PermissionGrant>,
    #[serde(skip)]
    pub(crate) changed: std::sync::Arc<tokio::sync::Notify>,
    #[serde(default, rename = "extraHTTPHeaders")]
    pub(crate) extra_http_headers: std::collections::BTreeMap<String, String>,
    pub(crate) http_credentials: Option<turbo_surf_core::http_auth::Credentials>,
}

impl BrowserEmulation {
    pub(crate) fn request_headers(
        &self,
        authored: std::collections::BTreeMap<String, String>,
    ) -> std::collections::BTreeMap<String, String> {
        let mut headers = std::collections::BTreeMap::new();
        if let Some(locale) = &self.locale {
            headers.insert("accept-language".to_string(), locale.clone());
        }
        headers.extend(
            self.extra_http_headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone())),
        );
        headers.extend(
            authored
                .into_iter()
                .map(|(key, value)| (key.to_ascii_lowercase(), value)),
        );
        headers
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.locale.as_ref().is_some_and(|locale| {
            locale.is_empty()
                || !locale
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }) {
            return Err("Invalid locale".into());
        }
        if self
            .color_scheme
            .as_deref()
            .is_some_and(|s| !["dark", "light", "no-preference"].contains(&s))
        {
            return Err("Invalid colorScheme".into());
        }
        if self
            .reduced_motion
            .as_deref()
            .is_some_and(|s| !["reduce", "no-preference"].contains(&s))
        {
            return Err("Invalid reducedMotion".into());
        }
        if let Some(position) = &self.geolocation {
            position.validate()?;
        }
        for grant in &self.permissions {
            grant.validate()?;
        }
        if let Some(credentials) = &self.http_credentials {
            credentials.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_validate_before_replacing_live_state() {
        assert!(serde_json::from_str::<BrowserEmulation>(
            r#"{"colorScheme":"dark","reducedMotion":"reduce"}"#
        )
        .unwrap()
        .validate()
        .is_ok());
        assert!(
            serde_json::from_str::<BrowserEmulation>(r#"{"colorScheme":"unknown"}"#)
                .unwrap()
                .validate()
                .is_err()
        );
        assert!(
            serde_json::from_str::<BrowserEmulation>(r#"{"reducedMotion":"fast"}"#)
                .unwrap()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn tab_preferences_are_isolated_and_session_clones_share_updates() {
        let profile = crate::runtime::BrowserStorage::default();
        profile
            .configure_emulation(r#"{"colorScheme":"dark"}"#)
            .unwrap();
        let first = profile.new_tab().unwrap();
        let second = profile.new_tab().unwrap();
        let session = first.clone();
        first
            .configure_emulation(r#"{"colorScheme":"light"}"#)
            .unwrap();
        assert_eq!(session.emulation().color_scheme.as_deref(), Some("light"));
        assert_eq!(second.emulation().color_scheme.as_deref(), Some("dark"));
        assert_eq!(profile.emulation().color_scheme.as_deref(), Some("dark"));
        assert!(first
            .configure_emulation(r#"{"colorScheme":"invalid"}"#)
            .is_err());
        assert_eq!(first.emulation().color_scheme.as_deref(), Some("light"));
    }
    #[test]
    fn locale_defaults_are_per_context_and_authored_language_still_wins() {
        let options = BrowserEmulation {
            locale: Some("fr-FR".into()),
            ..Default::default()
        };
        assert_eq!(
            options
                .request_headers(Default::default())
                .get("accept-language")
                .map(String::as_str),
            Some("fr-FR")
        );
        let authored =
            std::collections::BTreeMap::from([("Accept-Language".into(), "de-DE".into())]);
        assert_eq!(
            options
                .request_headers(authored)
                .get("accept-language")
                .map(String::as_str),
            Some("de-DE")
        );
        assert!(BrowserEmulation {
            locale: Some("fr_FR".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
    #[test]
    fn extra_headers_reach_transport_with_case_insensitive_request_overrides() {
        let options = BrowserEmulation {
            extra_http_headers: std::collections::BTreeMap::from([(
                "X-Probe".into(),
                "context".into(),
            )]),
            ..Default::default()
        };
        assert_eq!(
            options
                .request_headers(Default::default())
                .get("x-probe")
                .map(String::as_str),
            Some("context")
        );
        let authored = std::collections::BTreeMap::from([("x-PROBE".into(), "request".into())]);
        let headers = options.request_headers(authored);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers.get("x-probe").map(String::as_str), Some("request"));
    }
}
