//! Native source comparison normalizes browser line endings without rewriting code units.
use super::normalized_frame_source;

#[test]
fn source_authorization_normalizes_crlf_but_preserves_body_whitespace_and_utf16() {
    for (authored, expected) in [
        (
            " \t\r\n  const value = '😀';\r\n\r\n// tail\r",
            "const value = '😀';\n\n// tail\n",
        ),
        ("const n=1;\r\nconst m=2;\n", "const n=1;\nconst m=2;\n"),
        ("a\u{2028}b\u{2029}😀", "a\u{2028}b\u{2029}😀"),
    ] {
        let units = authored.encode_utf16().collect::<Vec<_>>();
        let original = units.clone();
        let normalized = normalized_frame_source(&units);
        assert_eq!(normalized, expected.encode_utf16().collect::<Vec<_>>());
        assert_eq!(
            units, original,
            "comparison must not mutate the fetched body"
        );
        assert_eq!(normalized_frame_source(&normalized), normalized);
    }
}
