//! Minimal AWS Signature Version 4 signing (pure Rust — portable to wasm32).
//!
//! Lets WASM components call AWS REST APIs (SageMaker, S3, …) directly over
//! outbound HTTPS without the native SDK. Only the signing math lives here;
//! HTTP transport is the caller's concern.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub struct SigningParams<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    /// `YYYYMMDDTHHMMSSZ`
    pub amz_date: &'a str,
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 "unreserved characters" encoding used by SigV4.
fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Compute the SigV4 `Authorization` header value.
///
/// `headers` must contain every header to be signed (at minimum `host` and
/// `x-amz-date`, matching `amz_date`); names are lowercased for signing.
/// `canonical_uri` is the absolute path (already-decoded), `query` is a list
/// of raw key/value pairs.
pub fn authorization_header(
    params: &SigningParams<'_>,
    method: &str,
    canonical_uri: &str,
    query: &[(String, String)],
    headers: &[(String, String)],
    payload_sha256_hex: &str,
) -> String {
    // canonical query string: encoded, sorted by key then value
    let mut encoded_query: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    encoded_query.sort();
    let canonical_query = encoded_query
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    // canonical headers: lowercase names, trimmed values, sorted
    let mut normalized: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_string()))
        .collect();
    normalized.sort();
    let canonical_headers = normalized
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let signed_headers = normalized
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{method}\n{}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_sha256_hex}",
        uri_encode(canonical_uri, false),
    );

    let date = &params.amz_date[..8];
    let credential_scope = format!("{date}/{}/{}/aws4_request", params.region, params.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{credential_scope}\n{}",
        params.amz_date,
        sha256_hex(canonical_request.as_bytes()),
    );

    let k_date = hmac(format!("AWS4{}", params.secret_key).as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, params.region.as_bytes());
    let k_service = hmac(&k_region, params.service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        params.access_key,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `get-vanilla-query-order-key-case`-style example from the AWS
    /// SigV4 documentation ("Task 1-4" worked example, ListUsers on IAM).
    #[test]
    fn matches_aws_documentation_example() {
        let params = SigningParams {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            region: "us-east-1",
            service: "iam",
            amz_date: "20150830T123600Z",
        };
        let auth = authorization_header(
            &params,
            "GET",
            "/",
            &[
                ("Action".to_string(), "ListUsers".to_string()),
                ("Version".to_string(), "2010-05-08".to_string()),
            ],
            &[
                (
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded; charset=utf-8".to_string(),
                ),
                ("host".to_string(), "iam.amazonaws.com".to_string()),
                ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
            ],
            &sha256_hex(b""),
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }
}
