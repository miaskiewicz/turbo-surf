use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Geolocation {
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
    #[serde(default)]
    pub(crate) accuracy: f64,
}
impl Geolocation {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (name, value, limit) in [
            ("latitude", self.latitude, 90.0),
            ("longitude", self.longitude, 180.0),
        ] {
            if !value.is_finite() || value.abs() > limit {
                return Err(format!("Invalid geolocation {name}"));
            }
        }
        if !self.accuracy.is_finite() || self.accuracy < 0.0 {
            return Err("Invalid geolocation accuracy".into());
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct PermissionGrant {
    pub(crate) origin: Option<String>,
    pub(crate) permissions: Vec<String>,
}
impl PermissionGrant {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if let Some(origin) = &self.origin {
            let url = url::Url::parse(origin).map_err(|_| "Invalid permission origin")?;
            if url.origin().ascii_serialization() != *origin || origin == "null" {
                return Err("Invalid permission origin".into());
            }
        }
        if self
            .permissions
            .iter()
            .any(|name| !PERMISSIONS.contains(&name.as_str()))
        {
            return Err("Unsupported context permission".into());
        }
        Ok(())
    }
}

pub(crate) fn granted(grants: &[PermissionGrant], origin: &str, name: &str) -> bool {
    grants.iter().any(|grant| {
        grant.origin.as_ref().is_none_or(|scope| scope == origin)
            && grant
                .permissions
                .iter()
                .any(|permission| permission == name)
    })
}

const PERMISSIONS: &[&str] = &[
    "geolocation",
    "midi",
    "midi-sysex",
    "notifications",
    "camera",
    "microphone",
    "background-sync",
    "ambient-light-sensor",
    "accelerometer",
    "gyroscope",
    "magnetometer",
    "accessibility-events",
    "clipboard-read",
    "clipboard-write",
    "payment-handler",
    "persistent-storage",
    "idle-detection",
    "screen-wake-lock",
    "local-fonts",
    "storage-access",
    "top-level-storage-access",
    "window-management",
];
