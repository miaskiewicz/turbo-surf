//! Host-provided HTTP authentication, scoped independently of authored headers.
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub origin: Option<String>,
}

impl Credentials {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .origin
            .as_ref()
            .is_some_and(|origin| crate::request_context::origin(origin).is_none())
        {
            return Err("HTTP credential origin must be an HTTP(S) origin".into());
        }
        Ok(())
    }
    pub(crate) fn authorization(&self, url: &str) -> Option<String> {
        if self
            .origin
            .as_ref()
            .is_some_and(|origin| !crate::request_context::same_origin(origin, url))
        {
            return None;
        }
        let value = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.username, self.password));
        Some(format!("Basic {value}"))
    }
}

pub(crate) fn basic_challenge(value: &str) -> bool {
    value.split(',').any(|challenge| {
        let challenge = challenge.trim_start();
        challenge
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("Basic"))
            && challenge
                .as_bytes()
                .get(5)
                .is_none_or(u8::is_ascii_whitespace)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_credentials_are_origin_scoped_and_require_an_actual_challenge_scheme() {
        let credentials = Credentials {
            username: "alice".into(),
            password: "secret".into(),
            origin: Some("http://example.test:8080".into()),
        };
        assert_eq!(
            credentials
                .authorization("http://example.test:8080/path")
                .as_deref(),
            Some("Basic YWxpY2U6c2VjcmV0")
        );
        assert!(credentials
            .authorization("http://example.test:8081/path")
            .is_none());
        assert!(credentials
            .authorization("https://example.test:8080/path")
            .is_none());
        assert!(basic_challenge("Digest realm=one, bAsIc realm=two"));
        assert!(!basic_challenge("Bearer token"));
        assert!(!basic_challenge("BasicSomething realm=one"));
        assert!(credentials.validate().is_ok());
    }
}
