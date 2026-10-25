//! Immutable native navigation plans; no guest objects survive form admission.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Submission {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// A File entry's native snapshot: its name, type and bytes, never the guest object.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FilePart {
    pub name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Encoding accepts the entry list after successful-controls construction. Modern
/// HTML form submission selects its native document encoding before normalization.
#[allow(clippy::too_many_arguments, dead_code)]
pub(super) fn prepare(
    document_url: &str,
    resolved_action: &str,
    method: &str,
    enctype: &str,
    target: &str,
    entries: &[(String, String)],
    charset_entries: &[usize],
    encoding: &'static encoding_rs::Encoding,
) -> Result<Submission, String> {
    prepare_with_files(
        document_url,
        resolved_action,
        method,
        enctype,
        target,
        entries,
        &[],
        charset_entries,
        encoding,
    )
}

/// As [`prepare`]; `files[i]` holds the File snapshot of entry `i` (whose text is
/// then its file name), which multipart/form-data encodes as a file part.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_with_files(
    document_url: &str,
    resolved_action: &str,
    method: &str,
    enctype: &str,
    target: &str,
    entries: &[(String, String)],
    files: &[Option<FilePart>],
    charset_entries: &[usize],
    encoding: &'static encoding_rs::Encoding,
) -> Result<Submission, String> {
    if !target.is_empty() && !target.eq_ignore_ascii_case("_self") {
        return Err("Native form submission to another browsing context is not supported".into());
    }
    let mut url = url::Url::parse(if resolved_action.is_empty() {
        document_url
    } else {
        resolved_action
    })
    .map_err(|_| "Native form action is not an absolute URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Native form submission requires an HTTP(S) action".into());
    }
    let entries = entries
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            let value = if charset_entries.contains(&index) {
                encoding.name()
            } else {
                value
            };
            (newlines(name), newlines(value))
        })
        .collect::<Vec<_>>();
    let encoded = || {
        entries
            .iter()
            .map(|(name, value)| format!("{}={}", encode(name, encoding), encode(value, encoding)))
            .collect::<Vec<_>>()
            .join("&")
    };
    let mut submission = Submission {
        url: String::new(),
        method: method.to_ascii_uppercase(),
        headers: BTreeMap::new(),
        body: None,
    };
    match method {
        "get" => url.set_query(Some(&encoded())),
        "post" if enctype == "multipart/form-data" => {
            let boundary = boundary()?;
            submission.headers.insert(
                "content-type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            );
            submission.body = Some(multipart(&entries, files, encoding, &boundary));
        }
        "post" => {
            let body = match enctype {
                "application/x-www-form-urlencoded" => encoded(),
                "text/plain" => entries
                    .iter()
                    .map(|(name, value)| format!("{name}={value}\r\n"))
                    .collect::<String>(),
                _ => return Err("Native form encoding is not supported".into()),
            };
            submission
                .headers
                .insert("content-type".into(), enctype.into());
            submission.body = Some(if enctype == "text/plain" {
                encoding.encode(&body).0.into_owned()
            } else {
                body.into_bytes()
            });
        }
        _ => return Err("Native form method is not supported".into()),
    }
    submission.url = url.into();
    Ok(submission)
}

fn newlines(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push_str("\r\n");
            }
            '\n' => result.push_str("\r\n"),
            ch => result.push(ch),
        }
    }
    result
}

fn encode(value: &str, encoding: &'static encoding_rs::Encoding) -> String {
    let (bytes, _, _) = encoding.encode(value);
    let mut result = String::new();
    for byte in bytes.iter().copied() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                result.push(byte as char)
            }
            b' ' => result.push('+'),
            byte => {
                use std::fmt::Write;
                write!(&mut result, "%{byte:02X}").unwrap();
            }
        }
    }
    result
}

/// Blink's boundary: a fixed prefix and 16 characters from its 64-entry alphabet.
fn boundary() -> Result<String, String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789AB";
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| "Native multipart boundary entropy unavailable")?;
    let mut boundary = String::from("----WebKitFormBoundary");
    boundary.extend(
        random
            .iter()
            .map(|byte| ALPHABET[usize::from(byte & 63)] as char),
    );
    Ok(boundary)
}

/// A Content-Disposition quoted-string: Blink percent-escapes only `"`, CR and LF.
fn quoted(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        match byte {
            b'"' => out.extend_from_slice(b"%22"),
            b'\r' => out.extend_from_slice(b"%0D"),
            b'\n' => out.extend_from_slice(b"%0A"),
            byte => out.push(byte),
        }
    }
}

/// multipart/form-data over newline-normalized names and text values in the form's
/// encoding (HTML character references for unmappable characters). File names keep
/// their line breaks; their bytes are copied unchanged.
pub(super) fn multipart(
    entries: &[(String, String)],
    files: &[Option<FilePart>],
    encoding: &'static encoding_rs::Encoding,
    boundary: &str,
) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, (name, value)) in entries.iter().enumerate() {
        out.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"").as_bytes(),
        );
        quoted(&mut out, &encoding.encode(name).0);
        out.push(b'"');
        match files.get(index).and_then(Option::as_ref) {
            Some(file) => {
                out.extend_from_slice(b"; filename=\"");
                quoted(&mut out, &encoding.encode(&file.name).0);
                let content_type = if file.content_type.is_empty() {
                    "application/octet-stream"
                } else {
                    &file.content_type
                };
                out.extend_from_slice(
                    format!("\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
                );
                out.extend_from_slice(&file.bytes);
            }
            None => {
                out.extend_from_slice(b"\r\n\r\n");
                out.extend_from_slice(&encoding.encode(value).0);
            }
        }
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}
