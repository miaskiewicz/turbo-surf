//! Facts a document keeps from the navigation response that created it.
use std::collections::HashMap;

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    headers
        .iter()
        .filter(|(field, _)| field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect()
}

/// The response's `Last-Modified` instant in ms since the epoch. An absent or
/// unparsable date leaves `document.lastModified` at the current time, as Chrome does.
pub(crate) fn last_modified(headers: &[(String, String)]) -> Option<f64> {
    let value = header(headers, "last-modified").into_iter().next()?;
    let instant = httpdate::parse_http_date(value.trim()).ok()?;
    let since = instant.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_millis() as f64)
}

/// `Origin-Agent-Cluster` is a structured-field boolean; anything else is no request.
fn requested_keying(headers: &[(String, String)]) -> Option<bool> {
    let values = header(headers, "origin-agent-cluster");
    if values.is_empty() {
        return None;
    }
    sfv::Parser::new(&values.join(", "))
        .parse::<sfv::Item>()
        .ok()?
        .bare_item
        .as_boolean()
}

/// HTML's historical agent cluster key map for one browsing context group: the
/// first document of an origin fixes whether that origin is origin-keyed, and
/// later documents of the origin keep that keying whatever their headers request.
#[derive(Default)]
pub(crate) struct AgentClusters(HashMap<String, bool>);
impl AgentClusters {
    /// Opaque origins are always origin-keyed. `default` is Chrome's decision for a
    /// response without the header: potentially trustworthy origins are keyed.
    pub(crate) fn keyed(
        &mut self,
        url: &str,
        opaque: bool,
        headers: &[(String, String)],
        default: bool,
    ) -> bool {
        let origin = url::Url::parse(url)
            .map(|url| url.origin())
            .ok()
            .filter(|origin| origin.is_tuple() && !opaque);
        let Some(origin) = origin else {
            return true;
        };
        *self
            .0
            .entry(origin.ascii_serialization())
            .or_insert_with(|| requested_keying(headers).unwrap_or(default))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn last_modified_reads_http_dates_only() {
        let at = |value| last_modified(&fields(&[("Last-Modified", value)]));
        assert_eq!(
            at("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(1_445_412_480_000.0)
        );
        assert_eq!(at("yesterday"), None);
        assert_eq!(last_modified(&[]), None);
    }

    #[test]
    fn first_document_of_an_origin_fixes_its_keying() {
        let mut clusters = AgentClusters::default();
        let off = fields(&[("Origin-Agent-Cluster", "?0")]);
        let on = fields(&[("origin-agent-cluster", "?1")]);
        assert!(!clusters.keyed("http://127.0.0.1:1/a", false, &off, true));
        assert!(!clusters.keyed("http://127.0.0.1:1/b", false, &on, true));
        assert!(clusters.keyed("http://localhost:1/", false, &[], true));
        assert!(clusters.keyed("http://localhost:1/x", false, &off, true));
        assert!(clusters.keyed("http://127.0.0.1:1/", true, &off, true));
        assert!(clusters.keyed("data:text/html,x", false, &off, false));
        let invalid = fields(&[
            ("Origin-Agent-Cluster", "?0"),
            ("Origin-Agent-Cluster", "?1"),
        ]);
        assert!(clusters.keyed("http://example.test/", false, &invalid, true));
    }
}
