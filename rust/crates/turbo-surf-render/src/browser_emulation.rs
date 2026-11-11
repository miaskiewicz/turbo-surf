use serde::{Deserialize, Serialize};

/// Host-owned context preferences; cloned for each new tab and updated through
/// native request-state handles, never writable by guest page scripts.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserEmulation {
    pub(crate) color_scheme: Option<String>,
    pub(crate) reduced_motion: Option<String>,
}

impl BrowserEmulation {
    pub(crate) fn validate(&self) -> Result<(), String> {
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
}
