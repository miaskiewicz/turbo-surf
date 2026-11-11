pub(crate) fn file_mime_type(path: &str) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_paths_use_case_insensitive_registered_types_and_binary_fallback() {
        assert_eq!(file_mime_type("fixture.CSV"), "text/csv");
        assert_eq!(file_mime_type("certificate.pdf"), "application/pdf");
        assert_eq!(file_mime_type("picture.png"), "image/png");
        assert_eq!(
            file_mime_type("unknown.unregistered-surf"),
            "application/octet-stream"
        );
    }
}
