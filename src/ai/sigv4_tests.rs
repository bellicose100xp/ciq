use super::*;
use crate::ai::aws_creds::Credentials;

/// The official AWS SigV4 test-suite fixtures.
const SUITE_EPOCH: u64 = 1_440_938_160;
const SUITE_DATE: &str = "20150830T123600Z";
const SUITE_HOST: &str = "example.amazonaws.com";

fn suite_creds(session_token: Option<&str>) -> Credentials {
    Credentials {
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
        session_token: session_token.map(str::to_owned),
    }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn suite_request<'a>(
    method: &'a str,
    query: &'a [(String, String)],
    headers: &'a [(String, String)],
    body: &'a [u8],
) -> SignRequest<'a> {
    SignRequest {
        method,
        host: SUITE_HOST,
        path: "/",
        query,
        headers,
        body,
        region: "us-east-1",
        service: "service",
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("missing header {name}"))
}

fn authorization_parts(auth: &str) -> (String, String, String) {
    let rest = auth
        .strip_prefix("AWS4-HMAC-SHA256 ")
        .expect("algorithm prefix");
    let mut credential = None;
    let mut signed = None;
    let mut signature = None;
    for part in rest.split(", ") {
        if let Some(v) = part.strip_prefix("Credential=") {
            credential = Some(v.to_owned());
        } else if let Some(v) = part.strip_prefix("SignedHeaders=") {
            signed = Some(v.to_owned());
        } else if let Some(v) = part.strip_prefix("Signature=") {
            signature = Some(v.to_owned());
        }
    }
    (
        credential.expect("credential"),
        signed.expect("signed headers"),
        signature.expect("signature"),
    )
}

#[test]
fn amz_date_matches_suite_timestamp() {
    assert_eq!(amz_date(SUITE_EPOCH), SUITE_DATE);
}

#[test]
fn amz_date_handles_leap_day() {
    assert_eq!(amz_date(1_709_164_800), "20240229T000000Z");
}

#[test]
fn amz_date_epoch_zero() {
    assert_eq!(amz_date(0), "19700101T000000Z");
}

#[test]
fn amz_date_end_of_year_and_century_boundaries() {
    // 1999-12-31T23:59:59Z
    assert_eq!(amz_date(946_684_799), "19991231T235959Z");
    // 2000-01-01T00:00:00Z (a leap year divisible by 400)
    assert_eq!(amz_date(946_684_800), "20000101T000000Z");
    // 2100-03-01T00:00:00Z: 2100 is not a leap year
    assert_eq!(amz_date(4_107_542_400), "21000301T000000Z");
}

#[test]
fn sha256_hex_of_empty_and_known_input() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn get_vanilla_vector() {
    let req = suite_request("GET", &[], &[], b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    assert_eq!(header(&out, "x-amz-date"), SUITE_DATE);
    assert!(out.iter().all(|(k, _)| k != "x-amz-security-token"));
    let (credential, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(
        credential,
        "AKIDEXAMPLE/20150830/us-east-1/service/aws4_request"
    );
    assert_eq!(signed, "host;x-amz-date");
    assert_eq!(
        signature,
        "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

#[test]
fn post_vanilla_vector() {
    let req = suite_request("POST", &[], &[], b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "host;x-amz-date");
    assert_eq!(
        signature,
        "5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b"
    );
}

#[test]
fn post_x_www_form_urlencoded_vector() {
    let headers = pairs(&[("content-type", "application/x-www-form-urlencoded")]);
    let req = suite_request("POST", &[], &headers, b"Param1=value1");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "content-type;host;x-amz-date");
    assert_eq!(
        signature,
        "ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a"
    );
}

#[test]
fn get_vanilla_query_order_key_case_vector() {
    let query = pairs(&[("Param2", "value2"), ("Param1", "value1")]);
    let req = suite_request("GET", &query, &[], b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, _, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(
        signature,
        "b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
    );
}

#[test]
fn session_token_is_emitted_and_signed() {
    let req = suite_request("GET", &[], &[], b"");
    let out = sign(&req, &suite_creds(Some("TOKEN123")), SUITE_EPOCH);
    assert_eq!(header(&out, "x-amz-security-token"), "TOKEN123");
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "host;x-amz-date;x-amz-security-token");
    // The token participates in the signature, so it must differ from the token-less vector.
    assert_ne!(
        signature,
        "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

#[test]
fn emitted_header_order_is_date_token_authorization() {
    let req = suite_request("GET", &[], &[], b"");
    let out = sign(&req, &suite_creds(Some("t")), SUITE_EPOCH);
    let names: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names,
        ["x-amz-date", "x-amz-security-token", "authorization"]
    );
}

#[test]
fn canonical_request_get_vanilla_text() {
    let req = suite_request("GET", &[], &[], b"");
    let headers = pairs(&[("x-amz-date", SUITE_DATE), ("host", SUITE_HOST)]);
    let expected = format!(
        "GET\n/\n\nhost:{SUITE_HOST}\nx-amz-date:{SUITE_DATE}\n\nhost;x-amz-date\n{}",
        sha256_hex(b"")
    );
    assert_eq!(canonical_request(&req, &headers), expected);
}

#[test]
fn canonical_request_sorts_and_encodes_query() {
    let query = pairs(&[("b", "2"), ("a", "x y"), ("a", "1")]);
    let req = suite_request("GET", &query, &[], b"");
    let canonical = canonical_request(&req, &[]);
    let query_line = canonical.lines().nth(2).expect("query line");
    assert_eq!(query_line, "a=1&a=x%20y&b=2");
}

#[test]
fn canonical_request_encodes_path_segments_keeping_slashes() {
    let req = SignRequest {
        method: "POST",
        host: SUITE_HOST,
        path: "/model/anthropic.claude v1:0/invoke",
        query: &[],
        headers: &[],
        body: b"{}",
        region: "us-east-1",
        service: "bedrock",
    };
    let canonical = canonical_request(&req, &[]);
    let path_line = canonical.lines().nth(1).expect("path line");
    assert_eq!(path_line, "/model/anthropic.claude%20v1%3A0/invoke");
}

#[test]
fn canonical_request_empty_path_is_root() {
    let req = SignRequest {
        method: "GET",
        host: SUITE_HOST,
        path: "",
        query: &[],
        headers: &[],
        body: b"",
        region: "us-east-1",
        service: "service",
    };
    let canonical = canonical_request(&req, &[]);
    assert_eq!(canonical.lines().nth(1), Some("/"));
}

#[test]
fn header_names_lowercased_values_trimmed_and_collapsed() {
    let headers = pairs(&[("Content-Type", "  application/json   ;  charset=utf-8 ")]);
    let req = suite_request("POST", &[], &headers, b"{}");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, _) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "content-type;host;x-amz-date");

    let mut signed = headers.clone();
    signed.push(("host".to_owned(), SUITE_HOST.to_owned()));
    let canonical = canonical_request(&req, &signed);
    assert!(
        canonical.contains("content-type:application/json ; charset=utf-8\n"),
        "{canonical}"
    );
}

#[test]
fn caller_supplied_host_is_not_duplicated() {
    let headers = pairs(&[("Host", SUITE_HOST)]);
    let req = suite_request("GET", &[], &headers, b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "host;x-amz-date");
    assert_eq!(
        signature,
        "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

#[test]
fn string_to_sign_layout() {
    let s = string_to_sign(SUITE_DATE, "20150830/us-east-1/service/aws4_request", "abc");
    assert_eq!(
        s,
        "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\nabc"
    );
}

#[test]
fn signature_changes_with_body() {
    let req_a = suite_request("POST", &[], &[], b"a");
    let req_b = suite_request("POST", &[], &[], b"b");
    let creds = suite_creds(None);
    let sig_a = authorization_parts(header(&sign(&req_a, &creds, SUITE_EPOCH), "authorization")).2;
    let sig_b = authorization_parts(header(&sign(&req_b, &creds, SUITE_EPOCH), "authorization")).2;
    assert_ne!(sig_a, sig_b);
}

#[test]
fn signature_changes_with_region_and_service_scope() {
    let creds = suite_creds(None);
    let base = suite_request("GET", &[], &[], b"");
    let other = SignRequest {
        region: "eu-west-1",
        ..suite_request("GET", &[], &[], b"")
    };
    let out = sign(&other, &creds, SUITE_EPOCH);
    let (credential, _, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(
        credential,
        "AKIDEXAMPLE/20150830/eu-west-1/service/aws4_request"
    );
    let base_sig =
        authorization_parts(header(&sign(&base, &creds, SUITE_EPOCH), "authorization")).2;
    assert_ne!(signature, base_sig);
}

#[test]
fn get_header_key_duplicate_vector() {
    // Repeated headers canonicalize to one entry, values comma-joined in arrival order.
    let headers = pairs(&[
        ("My-Header1", "value2"),
        ("My-Header1", "value2"),
        ("My-Header1", "value1"),
    ]);
    let req = suite_request("GET", &[], &headers, b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "host;my-header1;x-amz-date");
    assert_eq!(
        signature,
        "c9d5ea9f3f72853aea855b47ea873832890dbdd183b4468f858259531a5138ea"
    );
}

#[test]
fn get_header_value_trim_vector() {
    // Surrounding whitespace is trimmed and runs collapse, quotes included (the suite's canonical
    // request reads `my-header2:"a b c"`).
    let headers = pairs(&[("My-Header1", " value1"), ("My-Header2", " \"a   b   c\"")]);
    let req = suite_request("GET", &[], &headers, b"");
    let out = sign(&req, &suite_creds(None), SUITE_EPOCH);
    let (_, signed, signature) = authorization_parts(header(&out, "authorization"));
    assert_eq!(signed, "host;my-header1;my-header2;x-amz-date");
    assert_eq!(
        signature,
        "acc3ed3afb60bb290fc8d2dd0098b9911fcaa05412b367055dee359757a9c736"
    );
}

#[test]
fn header_values_collapse_tabs_and_folded_lines() {
    let headers = pairs(&[("X-Thing", "a\t\tb\n   c")]);
    let req = suite_request("GET", &[], &headers, b"");
    let canonical = canonical_request(&req, &headers);
    assert!(canonical.contains("x-thing:a b c\n"), "{canonical}");
}
