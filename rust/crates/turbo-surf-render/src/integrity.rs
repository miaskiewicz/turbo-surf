//! SRI hashes cover decoded response bytes, before any text conversion or execution.
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use sha2::{Digest, Sha256, Sha384, Sha512};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Algorithm {
    Sha256,
    Sha384,
    Sha512,
}

fn metadata(token: &str) -> Option<(Algorithm, Vec<u8>)> {
    let (algorithm, digest) = [
        ("sha256-", Algorithm::Sha256),
        ("sha-256-", Algorithm::Sha256),
        ("sha384-", Algorithm::Sha384),
        ("sha-384-", Algorithm::Sha384),
        ("sha512-", Algorithm::Sha512),
        ("sha-512-", Algorithm::Sha512),
    ]
    .into_iter()
    .find_map(|(prefix, algorithm)| token.strip_prefix(prefix).map(|rest| (algorithm, rest)))?;
    let digest = digest.split('?').next().unwrap();
    if digest.is_empty()
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+/-_=".contains(&byte))
    {
        return None;
    }
    let normalized = digest.replace('-', "+").replace('_', "/");
    let decoder = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    // A syntactically admitted but undecodable digest still participates in
    // algorithm strength selection and fails verification, like Blink.
    Some((algorithm, decoder.decode(normalized).unwrap_or_default()))
}

pub fn matches(bytes: &[u8], integrity: &str, eligible: bool) -> bool {
    if integrity.is_empty() {
        return true;
    }
    if !eligible {
        return false;
    }
    let entries: Vec<_> = integrity
        .split_ascii_whitespace()
        .filter_map(metadata)
        .collect();
    let Some(strongest) = entries.iter().map(|(algorithm, _)| *algorithm).max() else {
        return true;
    };
    let actual = match strongest {
        Algorithm::Sha256 => Sha256::digest(bytes).to_vec(),
        Algorithm::Sha384 => Sha384::digest(bytes).to_vec(),
        Algorithm::Sha512 => Sha512::digest(bytes).to_vec(),
    };
    entries
        .iter()
        .any(|(algorithm, expected)| *algorithm == strongest && *expected == actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn strongest_metadata_controls_admission_and_equal_strength_is_disjunctive() {
        let bytes = b"response\xffbytes";
        let valid = format!("sha256-{}", STANDARD.encode(Sha256::digest(bytes)));
        let strong = format!("sha512-{}", STANDARD.encode(Sha512::digest(bytes)));
        assert!(matches(bytes, &valid, true));
        assert!(!matches(b"modified", &valid, true));
        assert!(!matches(bytes, &format!("{valid} sha512-YQ=="), true));
        assert!(matches(bytes, &format!("sha512-YQ== {strong}"), true));
        assert!(!matches(bytes, &format!("{valid} sha512-=="), true));
    }

    #[test]
    fn chrome_hash_aliases_urlsafe_padding_and_unknown_options_are_accepted() {
        let bytes = b"hello";
        let digest = STANDARD.encode(Sha384::digest(bytes));
        let urlsafe = digest.replace('+', "-").replace('/', "_");
        for integrity in [
            format!("sha384-{digest}"),
            format!("sha-384-{urlsafe}?future-option"),
            format!("\tsha384-{}\r\n", digest.trim_end_matches('=')),
        ] {
            assert!(matches(bytes, &integrity, true), "{integrity}");
        }
        for integrity in ["", "unknown-YQ==", "sha256-!invalid", "SHA256-YQ=="] {
            assert!(matches(bytes, integrity, true), "{integrity}");
        }
        assert!(!matches(bytes, "unknown-YQ==", false));
        assert!(matches(bytes, "", false));
    }
}
