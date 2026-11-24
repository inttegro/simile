use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub(crate) fn decode(content_type: &str, bytes: &[u8]) -> Result<Value> {
    let essence = essence(content_type);
    if is_json(essence) {
        return serde_json::from_slice(bytes).map_err(|error| Error::MalformedJson {
            line: error.line(),
            column: error.column(),
            message: error.to_string(),
        });
    }
    if essence.starts_with("text/") {
        return std::str::from_utf8(bytes)
            .map(|value| Value::String(value.to_owned()))
            .map_err(|error| malformed(content_type, error.to_string()));
    }
    match essence {
        "application/x-www-form-urlencoded" => Ok(decode_form(bytes)),
        "multipart/form-data" => decode_multipart(content_type, bytes),
        _ => Err(Error::UnsupportedEncoding {
            content_type: essence.to_owned(),
        }),
    }
}

pub(crate) fn encode(content_type: &str, value: &Value) -> Result<(String, Vec<u8>)> {
    let essence = essence(content_type);
    if is_json(essence) {
        return serde_json::to_vec(value)
            .map(|bytes| (essence.to_owned(), bytes))
            .map_err(|error| Error::EncodeBody {
                content_type: essence.to_owned(),
                message: error.to_string(),
            });
    }
    if essence.starts_with("text/") {
        return value
            .as_str()
            .map(|value| (essence.to_owned(), value.as_bytes().to_vec()))
            .ok_or_else(|| Error::EncodeBody {
                content_type: essence.to_owned(),
                message: format!("{essence} requires a string value"),
            });
    }
    match essence {
        "application/x-www-form-urlencoded" => Ok((
            essence.to_owned(),
            encode_form(value).map_err(|message| Error::EncodeBody {
                content_type: essence.to_owned(),
                message,
            })?,
        )),
        "multipart/form-data" => {
            let boundary = "simile-boundary";
            let bytes = encode_multipart(value, boundary).map_err(|message| Error::EncodeBody {
                content_type: essence.to_owned(),
                message,
            })?;
            Ok((format!("multipart/form-data; boundary={boundary}"), bytes))
        }
        _ => Err(Error::UnsupportedEncoding {
            content_type: essence.to_owned(),
        }),
    }
}

pub(crate) fn encode_response(content_type: &str, value: &Value) -> Result<(String, Vec<u8>)> {
    let essence = essence(content_type);
    let encoded = if is_json(essence) {
        serde_json::to_vec(value)
            .map(|bytes| (essence.to_owned(), bytes))
            .map_err(|error| error.to_string())
    } else if essence.starts_with("text/") || essence == "application/octet-stream" {
        value
            .as_str()
            .map(|value| (essence.to_owned(), value.as_bytes().to_vec()))
            .ok_or_else(|| format!("{essence} requires a string value"))
    } else if essence == "application/x-www-form-urlencoded" {
        encode_form(value).map(|bytes| (essence.to_owned(), bytes))
    } else if essence == "multipart/form-data" {
        let boundary = "simile-boundary";
        encode_multipart(value, boundary)
            .map(|bytes| (format!("multipart/form-data; boundary={boundary}"), bytes))
    } else {
        return Err(Error::UnsupportedResponseEncoding {
            content_type: essence.to_owned(),
        });
    };
    encoded.map_err(|message| Error::EncodeResponseBody {
        content_type: essence.to_owned(),
        message: message.to_string(),
    })
}

pub(crate) fn supports_response(content_type: &str) -> bool {
    let essence = essence(content_type);
    is_json(essence)
        || essence.starts_with("text/")
        || matches!(
            essence,
            "application/octet-stream"
                | "application/x-www-form-urlencoded"
                | "multipart/form-data"
        )
}

fn essence(content_type: &str) -> &str {
    content_type.split(';').next().map(str::trim).unwrap_or("")
}

fn is_json(media_type: &str) -> bool {
    media_type == "application/json" || media_type.ends_with("+json")
}

fn decode_form(bytes: &[u8]) -> Value {
    let mut fields = BTreeMap::<String, Vec<Value>>::new();
    for (name, value) in form_urlencoded::parse(bytes) {
        fields
            .entry(name.into_owned())
            .or_default()
            .push(Value::String(value.into_owned()));
    }
    Value::Object(
        fields
            .into_iter()
            .map(|(name, mut values)| {
                let value = if values.len() == 1 {
                    values.remove(0)
                } else {
                    Value::Array(values)
                };
                (name, value)
            })
            .collect(),
    )
}

fn encode_form(value: &Value) -> std::result::Result<Vec<u8>, String> {
    let fields = value
        .as_object()
        .ok_or_else(|| "form encoding requires an object value".to_owned())?;
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (name, value) in fields {
        match value {
            Value::Array(values) => {
                for value in values {
                    serializer.append_pair(name, &wire_scalar(value)?);
                }
            }
            value => {
                serializer.append_pair(name, &wire_scalar(value)?);
            }
        }
    }
    Ok(serializer.finish().into_bytes())
}

fn decode_multipart(content_type: &str, bytes: &[u8]) -> Result<Value> {
    let mut fields = BTreeMap::<String, Vec<Value>>::new();
    for part in multipart_parts(content_type, bytes)? {
        // A part with a filename is a file regardless of the file's own media
        // type. Treating `application/json` files as structured form fields
        // changes binary OpenAPI values into objects before validation.
        let value = if part.filename.is_some() {
            Value::String(STANDARD.encode(part.body))
        } else if part
            .content_type
            .is_some_and(|value| is_json(essence(value)))
        {
            serde_json::from_slice(part.body)
                .map_err(|error| malformed(content_type, error.to_string()))?
        } else {
            let text = std::str::from_utf8(part.body)
                .map_err(|error| malformed(content_type, error.to_string()))?;
            Value::String(text.to_owned())
        };
        fields.entry(part.name).or_default().push(value);
    }
    Ok(Value::Object(
        fields
            .into_iter()
            .map(|(name, mut values)| {
                let value = if values.len() == 1 {
                    values.remove(0)
                } else {
                    Value::Array(values)
                };
                (name, value)
            })
            .collect(),
    ))
}

/// Build replayable multipart evidence without retaining uploaded file bytes.
///
/// Ordinary fields are preserved unless the caller marks their names as sensitive. File
/// metadata remains available for diagnosis, while exact-length deterministic fixture bytes
/// replace the original content.
pub fn sanitize_multipart_for_replay<F>(
    content_type: &str,
    bytes: &[u8],
    is_sensitive: F,
) -> Result<Vec<u8>>
where
    F: Fn(&str) -> bool,
{
    let boundary = multipart_boundary(content_type)?;
    let mut output = Vec::new();
    for part in multipart_parts(content_type, bytes)? {
        output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        output.extend_from_slice(part.headers.as_bytes());
        if part.filename.is_some() {
            output.extend_from_slice(
                format!(
                    "\r\nX-Simile-Original-Length: {}\r\nX-Simile-Original-Sha256: {:x}\r\nX-Simile-Content: deterministic-replacement",
                    part.body.len(),
                    Sha256::digest(part.body)
                )
                .as_bytes(),
            );
        }
        output.extend_from_slice(b"\r\n\r\n");
        if part.filename.is_some() {
            output.extend_from_slice(&deterministic_file_bytes(part.body.len()));
        } else if is_sensitive(&part.name) {
            output.extend_from_slice(b"<redacted>");
        } else {
            output.extend_from_slice(part.body);
        }
        output.extend_from_slice(b"\r\n");
    }
    output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(output)
}

#[derive(Debug)]
struct MultipartPart<'a> {
    headers: &'a str,
    name: String,
    filename: Option<String>,
    content_type: Option<&'a str>,
    body: &'a [u8],
}

fn multipart_parts<'a>(content_type: &str, bytes: &'a [u8]) -> Result<Vec<MultipartPart<'a>>> {
    let boundary = multipart_boundary(content_type)?;
    let marker = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    for raw in split_bytes(bytes, &marker).into_iter().skip(1) {
        let raw = trim_trailing_crlf(trim_crlf(raw));
        if raw.is_empty() || raw == b"--" {
            continue;
        }
        let raw = raw.strip_suffix(b"--").unwrap_or(raw);
        let Some(header_end) = find_bytes(raw, b"\r\n\r\n") else {
            return Err(malformed(
                content_type,
                "multipart part has no header separator".to_owned(),
            ));
        };
        let headers = std::str::from_utf8(&raw[..header_end])
            .map_err(|error| malformed(content_type, error.to_string()))?;
        let disposition = header_value(headers, "content-disposition")
            .ok_or_else(|| malformed(content_type, "multipart part has no name".to_owned()))?;
        let name = parameter(disposition, "name")
            .ok_or_else(|| malformed(content_type, "multipart part has no name".to_owned()))?;
        parts.push(MultipartPart {
            headers,
            name,
            filename: parameter(disposition, "filename"),
            content_type: header_value(headers, "content-type"),
            body: trim_trailing_crlf(&raw[header_end + 4..]),
        });
    }
    Ok(parts)
}

fn multipart_boundary(content_type: &str) -> Result<String> {
    parameter(content_type, "boundary")
        .ok_or_else(|| malformed(content_type, "multipart boundary is missing".to_owned()))
}

fn deterministic_file_bytes(length: usize) -> Vec<u8> {
    const FIXTURE: &[u8] = b"simile-reproduction-fixture";
    (0..length)
        .map(|index| FIXTURE[index % FIXTURE.len()])
        .collect()
}

fn encode_multipart(value: &Value, boundary: &str) -> std::result::Result<Vec<u8>, String> {
    let fields = value
        .as_object()
        .ok_or_else(|| "multipart encoding requires an object value".to_owned())?;
    let mut output = Vec::new();
    for (name, value) in fields {
        let values = value
            .as_array()
            .map_or_else(|| vec![value], |values| values.iter().collect());
        for value in values {
            output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            output.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            output.extend_from_slice(wire_scalar(value)?.as_bytes());
            output.extend_from_slice(b"\r\n");
        }
    }
    output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(output)
}

fn wire_scalar(value: &Value) -> std::result::Result<String, String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).map_err(|error| error.to_string())
        }
    }
}

fn parameter(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn split_bytes<'a>(bytes: &'a [u8], delimiter: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    while let Some(offset) = find_bytes(&bytes[start..], delimiter) {
        parts.push(&bytes[start..start + offset]);
        start += offset + delimiter.len();
    }
    parts.push(&bytes[start..]);
    parts
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    (!needle.is_empty())
        .then(|| {
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        })
        .flatten()
}

fn trim_crlf(mut bytes: &[u8]) -> &[u8] {
    while let Some(rest) = bytes.strip_prefix(b"\r\n") {
        bytes = rest;
    }
    bytes
}

fn trim_trailing_crlf(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\r\n").unwrap_or(bytes)
}

fn malformed(content_type: &str, message: String) -> Error {
    Error::MalformedBody {
        content_type: essence(content_type).to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::json;

    use super::{STANDARD, decode, encode, sanitize_multipart_for_replay};

    #[test]
    fn url_encoded_forms_preserve_repeated_wire_values() {
        let decoded = decode(
            "application/x-www-form-urlencoded",
            b"name=Ama&quantity=2&tag=one&tag=two&active=true",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            decoded,
            json!({
                "name": "Ama",
                "quantity": "2",
                "tag": ["one", "two"],
                "active": "true"
            })
        );
    }

    #[test]
    fn multipart_forms_decode_named_parts() {
        let body = b"--abc\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nAma\r\n--abc\r\nContent-Disposition: form-data; name=\"quantity\"\r\n\r\n2\r\n--abc--\r\n";
        let decoded = decode("multipart/form-data; boundary=abc", body)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decoded, json!({"name": "Ama", "quantity": "2"}));
    }

    #[test]
    fn multipart_json_files_remain_binary_values() {
        let body = b"--abc\r\nContent-Disposition: form-data; name=\"file\"; filename=\"package.json\"\r\nContent-Type: application/json\r\n\r\n{\"name\":\"simile\"}\r\n--abc--\r\n";
        let decoded = decode("multipart/form-data; boundary=abc", body)
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(decoded, json!({"file": "eyJuYW1lIjoic2ltaWxlIn0="}));
    }

    #[test]
    fn multipart_report_evidence_preserves_fields_and_replaces_files() {
        let body = b"--abc\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nreport-repro\r\n--abc\r\nContent-Disposition: form-data; name=\"token\"\r\n\r\nsecret\r\n--abc\r\nContent-Disposition: form-data; name=\"file\"; filename=\"package.json\"\r\nContent-Type: application/json\r\n\r\n{\"name\":\"simile\"}\r\n--abc--\r\n";
        let sanitized =
            sanitize_multipart_for_replay("multipart/form-data; boundary=abc", body, |name| {
                name == "token"
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let rendered =
            String::from_utf8(sanitized.clone()).unwrap_or_else(|error| panic!("{error}"));

        assert!(rendered.contains("report-repro"));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("{\"name\":\"simile\"}"));
        assert!(rendered.contains("filename=\"package.json\""));
        assert!(rendered.contains("Content-Type: application/json"));
        assert!(rendered.contains("X-Simile-Original-Length: 17"));
        assert_eq!(
            decode("multipart/form-data; boundary=abc", &sanitized)
                .unwrap_or_else(|error| panic!("{error}"))["file"]
                .as_str()
                .and_then(|value| STANDARD.decode(value).ok())
                .map(|bytes| bytes.len()),
            Some(17)
        );
    }

    #[test]
    fn synthesized_forms_round_trip() {
        let value = json!({"name": "Ama", "quantity": "2"});
        let (content_type, bytes) = encode("application/x-www-form-urlencoded", &value)
            .unwrap_or_else(|error| panic!("{error}"));
        let decoded = decode(&content_type, &bytes).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decoded, value);
    }
}
