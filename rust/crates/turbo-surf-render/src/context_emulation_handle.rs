//! Send-only control over actual context preferences, independent of a Page alias.
use crate::browser_emulation::BrowserEmulation;
use std::sync::{Arc, RwLock};

/// Keeps context-wide preferences reachable while an auxiliary Page outlives its
/// opener. This handle owns neither a V8 realm nor a tab's storage namespace.
#[derive(Clone)]
pub struct ContextEmulationHandle(Arc<RwLock<BrowserEmulation>>);

impl ContextEmulationHandle {
    pub(crate) fn new(preferences: Arc<RwLock<BrowserEmulation>>) -> Self {
        Self(preferences)
    }

    /// Apply only context-wide location/permission settings. Per-page media and
    /// other existing preferences must survive updates after the opener closes.
    pub fn configure_geolocation(&self, json: &str) -> Result<(), String> {
        let options: BrowserEmulation =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        options.validate()?;
        let changed = {
            let mut current = self
                .0
                .write()
                .map_err(|_| "Context preferences unavailable")?;
            current.geolocation = options.geolocation;
            current.permissions = options.permissions;
            current.changed.clone()
        };
        changed.notify_waiters();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::BrowserStorage;

    #[test]
    fn retained_control_updates_actual_preferences_without_replacing_media_or_headers() {
        let storage = BrowserStorage::default();
        storage.configure_emulation(r#"{"colorScheme":"dark","reducedMotion":"reduce","locale":"fr-FR","extraHTTPHeaders":{"X-Probe":"retained"}}"#).unwrap();
        let control = storage.context_emulation_handle();
        let reader = storage.clone();
        drop(storage);
        control.configure_geolocation(r#"{"colorScheme":"light","reducedMotion":"no-preference","locale":"en-US","permissions":[{"permissions":["geolocation"]}],"geolocation":{"latitude":41,"longitude":-73,"accuracy":7}}"#).unwrap();
        reader.with_emulation(|options| {
            assert_eq!(options.color_scheme.as_deref(), Some("dark"));
            assert_eq!(options.reduced_motion.as_deref(), Some("reduce"));
            assert_eq!(options.locale.as_deref(), Some("fr-FR"));
            assert_eq!(
                options
                    .extra_http_headers
                    .get("X-Probe")
                    .map(String::as_str),
                Some("retained")
            );
            assert_eq!(options.geolocation.as_ref().unwrap().latitude, 41.0);
            assert!(crate::geolocation_config::granted(
                &options.permissions,
                "http://localhost",
                "geolocation"
            ));
        });
        assert!(control
            .configure_geolocation(r#"{"geolocation":{"latitude":100,"longitude":0}}"#)
            .is_err());
        assert_eq!(
            reader.with_emulation(|options| options.geolocation.as_ref().unwrap().latitude),
            41.0
        );
        control.configure_geolocation("{}").unwrap();
        assert!(reader.with_emulation(
            |options| options.geolocation.is_none() && options.permissions.is_empty()
        ));
    }
}
