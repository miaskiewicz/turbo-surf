//! Optional host-owned egress admission, fixed on first native network use.
//!
//! `TURBO_SURF_ALLOWED_ORIGINS` is a JSON array of exact HTTP(S)/WS(S) origins.
//! An absent variable preserves unrestricted browsing; invalid or empty policy
//! denies all network origins. Guest code cannot change the captured policy.
use std::collections::BTreeSet;
use std::sync::OnceLock;

const VARIABLE: &str = "TURBO_SURF_ALLOWED_ORIGINS";
const BLOCKED: &str = "Network origin blocked by host policy";
const INVALID: &str = "Invalid host network origin policy";

#[derive(Default)]
struct Policy {
    allowed: Option<Result<BTreeSet<String>, ()>>,
}

impl Policy {
    fn parse(value: Option<&str>) -> Self {
        Self {
            allowed: value.map(|value| {
                serde_json::from_str::<Vec<String>>(value)
                    .map_err(|_| ())?
                    .iter()
                    .map(|value| origin(value, true))
                    .collect()
            }),
        }
    }

    fn check(&self, target: &str) -> Result<(), &'static str> {
        match &self.allowed {
            None => Ok(()),
            Some(Err(())) => Err(INVALID),
            Some(Ok(allowed)) => origin(target, false)
                .ok()
                .filter(|target| allowed.contains(target))
                .map(|_| ())
                .ok_or(BLOCKED),
        }
    }
}

fn validate_origin_entry(value: &str) -> Result<(), ()> {
    let (_, authority) = value.split_once("://").ok_or(())?;
    if value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains('\\')
        || authority
            .find('/')
            .is_some_and(|index| &authority[index..] != "/")
    {
        return Err(());
    }
    Ok(())
}

fn origin(value: &str, entry: bool) -> Result<String, ()> {
    if entry {
        validate_origin_entry(value)?;
    }
    let url = url::Url::parse(value).map_err(|_| ())?;
    let valid = matches!(url.scheme(), "http" | "https" | "ws" | "wss")
        && url.host_str().is_some_and(|host| !host.contains('*'))
        && url.username().is_empty()
        && url.password().is_none()
        && (!entry || url.path() == "/" && url.query().is_none() && url.fragment().is_none());
    if !valid {
        return Err(());
    }
    Ok(url.origin().ascii_serialization())
}

fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| match std::env::var(VARIABLE) {
        Ok(value) => Policy::parse(Some(&value)),
        Err(std::env::VarError::NotPresent) => Policy::default(),
        Err(std::env::VarError::NotUnicode(_)) => Policy {
            allowed: Some(Err(())),
        },
    })
}

/// Recognize native admission failures without inspecting or repeating a URL.
pub fn is_denial(error: &str) -> bool {
    let message = error.strip_prefix("Network: ").unwrap_or(error);
    matches!(message, BLOCKED | INVALID)
}

/// Whether the host selected restricted networking, including invalid policy.
pub fn configured() -> bool {
    policy().allowed.is_some()
}

/// Admit a real network URL before cache access, name resolution or transport.
/// Error strings contain no URL, credentials, path or query material.
pub fn check(target: &str) -> Result<(), &'static str> {
    policy().check(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_policy_is_neutral_and_exact_origins_are_canonical() {
        assert!(Policy::parse(None).check("anything").is_ok());
        let policy = Policy::parse(Some(
            r#"["https://EXAMPLE.test:443","http://localhost:3301","ws://localhost:3301"]"#,
        ));
        for target in [
            "https://example.test/path?secret=value",
            "http://localhost:3301/",
            "ws://localhost:3301/socket",
        ] {
            assert!(policy.check(target).is_ok(), "{target}");
        }
        for target in [
            "https://example.test.evil/",
            "http://example.test/",
            "https://example.test:444/",
            "http://localhost:3302/",
            "wss://localhost:3301/",
            "https://user@example.test/",
            "data:text/plain,x",
        ] {
            assert_eq!(policy.check(target), Err(BLOCKED));
        }
    }

    #[test]
    fn invalid_or_empty_configuration_never_falls_back_to_unrestricted() {
        for value in [
            "",
            "null",
            "{}",
            "[1]",
            "not-json",
            r#"["https://*.test"]"#,
            r#"["https://user:secret@example.test"]"#,
            r#"["https://example.test/path"]"#,
            r#"["https://example.test/?secret=value"]"#,
            r#"["https://example.test/#fragment"]"#,
            r#"["file:///tmp"]"#,
            r#"["https://example.test/a/.."]"#,
            r#"["https://example.test/."]"#,
            r#"[" https://example.test"]"#,
            r#"["https://example.test","invalid"]"#,
        ] {
            assert_eq!(
                Policy::parse(Some(value)).check("https://example.test/"),
                Err(INVALID)
            );
        }
        assert_eq!(
            Policy::parse(Some("[]")).check("https://example.test/"),
            Err(BLOCKED)
        );
    }
}
