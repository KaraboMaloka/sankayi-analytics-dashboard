//! Secret scrubbing for anything that gets logged, stored, or shown in the UI.
//!
//! The provider clients pass credentials to Graph / Quicket as **query params**
//! (`access_token=…`, `api_key=…`). When a request fails at the transport layer
//! (DNS, timeout, TLS), reqwest's own error `Display` embeds the full request
//! URL — token and all. That error then flows into `ingestion_log.message` and
//! the Data Health page. `scrub_secrets` strips those values so a live token can
//! never land in the database, the `.log` files, or the browser.

/// Query-param names whose VALUES must never be shown. Matched case-insensitively.
const SECRET_PARAMS: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "usertoken",
    "client_secret",
    "token",
    "password",
];

/// Replace the value of every known secret query param (`name=VALUE`) with
/// `REDACTED`, and blank out `Bearer <token>` credentials. Everything else —
/// including the URL, the failing endpoint, and the real cause (e.g. "dns error:
/// os error 11001") — is preserved so the message stays diagnosable.
pub fn scrub_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for param in SECRET_PARAMS {
        out = scrub_param(&out, param);
    }
    scrub_bearer(&out)
}

/// Flatten an error's full source chain into one line, then scrub it. Use this
/// at the point a `reqwest::Error` is caught, so the token-bearing URL is gone
/// before the error is ever wrapped, logged, or returned from `main`.
pub fn scrub_error_chain<E: std::error::Error>(err: &E) -> String {
    let mut s = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        s.push_str(": ");
        s.push_str(&cause.to_string());
        source = cause.source();
    }
    scrub_secrets(&s)
}

/// A value ends at the first URL/query/quote delimiter (or whitespace).
fn is_value_delimiter(b: u8) -> bool {
    matches!(b, b'&' | b')' | b'"' | b'\'' | b',' | b' ' | b'\t' | b'\n' | b'\r')
}

/// Redact `param=VALUE` occurrences (case-insensitive on the name).
fn scrub_param(input: &str, param: &str) -> String {
    let haystack = input.to_ascii_lowercase();
    let needle = format!("{}=", param.to_ascii_lowercase());
    let bytes = input.as_bytes();

    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while let Some(rel) = haystack[i..].find(&needle) {
        let value_start = i + rel + needle.len(); // just past the '='
        out.push_str(&input[i..value_start]); // keep "param="
        let mut j = value_start;
        while j < bytes.len() && !is_value_delimiter(bytes[j]) {
            j += 1;
        }
        if j > value_start {
            out.push_str("REDACTED");
        }
        i = j;
    }
    out.push_str(&input[i..]);
    out
}

/// Redact `Bearer <token>` (Quicket's internal API auth header, in case an error
/// ever echoes it).
fn scrub_bearer(input: &str) -> String {
    let haystack = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let needle = "bearer ";

    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while let Some(rel) = haystack[i..].find(needle) {
        let value_start = i + rel + needle.len();
        out.push_str(&input[i..value_start]); // keep "Bearer " as written
        let mut j = value_start;
        while j < bytes.len() && !is_value_delimiter(bytes[j]) {
            j += 1;
        }
        if j > value_start {
            out.push_str("REDACTED");
        }
        i = j;
    }
    out.push_str(&input[i..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_access_token_in_url() {
        let s = "error sending request for url (https://graph.facebook.com/v26.0/123/media?access_token=EAGSTVZAT7YMoBS_secret): dns error: os error 11001";
        let out = scrub_secrets(s);
        assert!(!out.contains("EAGSTVZAT7YMoBS_secret"), "token leaked: {out}");
        assert!(out.contains("access_token=REDACTED"));
        // The diagnosable cause survives.
        assert!(out.contains("os error 11001"));
    }

    #[test]
    fn redacts_when_more_params_follow() {
        let s = "url (https://x/y?access_token=SECRET&fields=id,name&limit=50)";
        let out = scrub_secrets(s);
        assert!(!out.contains("SECRET"));
        assert!(out.contains("access_token=REDACTED"));
        assert!(out.contains("fields=id,name"), "non-secret params preserved: {out}");
    }

    #[test]
    fn redacts_api_key_and_bearer() {
        let s = "GET api_key=ABC123 Authorization: Bearer eyJhbGci.payload.sig done";
        let out = scrub_secrets(s);
        assert!(!out.contains("ABC123"));
        assert!(!out.contains("eyJhbGci.payload.sig"));
        assert!(out.contains("api_key=REDACTED"));
        assert!(out.contains("Bearer REDACTED"));
    }

    #[test]
    fn leaves_clean_strings_untouched() {
        let s = "posts: 114 posts, 1 account snapshot(s)";
        assert_eq!(scrub_secrets(s), s);
    }
}
