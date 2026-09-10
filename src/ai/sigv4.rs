//! AWS Signature Version 4 request signing, pure over an injected timestamp.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::ai::aws_creds::Credentials;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const TERMINATOR: &str = "aws4_request";

/// One HTTP request to sign. `path` and `query` are the raw (unencoded) forms; the signer applies
/// SigV4 URI encoding itself.
pub struct SignRequest<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub query: &'a [(String, String)],
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub region: &'a str,
    pub service: &'a str,
}

/// Format epoch seconds as the `YYYYMMDDTHHMMSSZ` value of `x-amz-date`.
pub fn amz_date(epoch_secs: u64) -> String {
    let days = epoch_secs / 86_400;
    let secs_of_day = epoch_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Sign `req` with `creds` at `epoch_secs`, returning the headers the caller must add:
/// `x-amz-date`, `x-amz-security-token` (when a session token is present) and `authorization`.
pub fn sign(req: &SignRequest<'_>, creds: &Credentials, epoch_secs: u64) -> Vec<(String, String)> {
    let date_time = amz_date(epoch_secs);
    let date = &date_time[..8];

    let mut added = vec![("x-amz-date".to_owned(), date_time.clone())];
    if let Some(token) = &creds.session_token {
        added.push(("x-amz-security-token".to_owned(), token.clone()));
    }

    let signed_headers = canonical_headers(req, &added);
    let canonical = canonical_request(req, &signed_headers);
    let scope = format!("{date}/{}/{}/{TERMINATOR}", req.region, req.service);
    let to_sign = string_to_sign(&date_time, &scope, &sha256_hex(canonical.as_bytes()));

    let signing_key = signing_key(&creds.secret_access_key, date, req.region, req.service);
    let signature = hex(&hmac(&signing_key, to_sign.as_bytes()));
    let header_names = signed_header_names(&signed_headers);

    added.push((
        "authorization".to_owned(),
        format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={header_names}, Signature={signature}",
            creds.access_key_id
        ),
    ));
    added
}

/// The SigV4 canonical request string. `signed_headers` are normalized (lowercased, trimmed) and
/// sorted here again so callers may pass them in any order.
pub fn canonical_request(req: &SignRequest<'_>, signed_headers: &[(String, String)]) -> String {
    let headers = normalize_headers(signed_headers);
    let header_block: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    format!(
        "{}\n{}\n{}\n{header_block}\n{}\n{}",
        req.method,
        canonical_path(req.path),
        canonical_query(req.query),
        signed_header_names(&headers),
        sha256_hex(req.body)
    )
}

/// The SigV4 string to sign.
pub fn string_to_sign(amz_date: &str, scope: &str, canonical_request_hash: &str) -> String {
    format!("{ALGORITHM}\n{amz_date}\n{scope}\n{canonical_request_hash}")
}

/// The caller's headers plus `host` and the signer-added headers, normalized and sorted.
fn canonical_headers(req: &SignRequest<'_>, added: &[(String, String)]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = req.headers.to_vec();
    headers.extend_from_slice(added);
    let has_host = headers
        .iter()
        .any(|(name, _)| name.trim().eq_ignore_ascii_case("host"));
    if !has_host {
        headers.push(("host".to_owned(), req.host.to_owned()));
    }
    normalize_headers(&headers)
}

/// Lowercase and trim names, collapse each value's whitespace, then merge duplicate names into one
/// entry whose value is the comma-joined values in arrival order (the SigV4 rule for repeated
/// headers), and sort by name.
fn normalize_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, value) in headers {
        let name = name.trim().to_ascii_lowercase();
        let value = collapse_whitespace(value);
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push(',');
                existing.push_str(&value);
            }
            None => out.push((name, value)),
        }
    }
    out.sort();
    out
}

fn signed_header_names(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";")
}

/// Trim the value and turn every run of whitespace (spaces, tabs, folded line breaks) into one
/// space. Quotes get no special treatment: the suite's `get-header-value-trim` vector canonicalizes
/// `"a   b   c"` to `"a b c"`, which is what the AWS SDKs do too.
fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// SigV4 URI encoding: unreserved `A-Za-z0-9-_.~` pass through, everything else is `%XX`.
fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, TERMINATOR.as_bytes())
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day). Howard Hinnant's
/// `civil_from_days`, valid for any u64 day count a timestamp can produce.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "sigv4_tests.rs"]
mod sigv4_tests;
