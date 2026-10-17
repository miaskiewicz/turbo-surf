//! Immutable native navigation plans; no guest objects survive form admission.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Submission {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Encoding accepts the entry list after successful-controls construction. Modern
/// HTML form submission selects its native document encoding before normalization.
#[allow(clippy::too_many_arguments)]
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
        "post" => {
            let body = match enctype {
                "application/x-www-form-urlencoded" => encoded(),
                "text/plain" => entries
                    .iter()
                    .map(|(name, value)| format!("{name}={value}\r\n"))
                    .collect::<String>(),
                _ => return Err("Native multipart form encoding is not supported".into()),
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
