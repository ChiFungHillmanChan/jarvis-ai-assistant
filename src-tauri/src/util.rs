//! Small shared string / HTTP utilities.

use reqwest::Client;
use std::sync::LazyLock;

/// Shared, connection-pooled HTTP client with connect + overall timeouts, so a
/// stalled connection (sleep/wake, captive portal, unresponsive backend) can
/// never hang a background cron job or a user action indefinitely.
static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| Client::new())
});

/// A process-wide timed HTTP client. Prefer this over `reqwest::Client::new()`
/// for integration and payment calls.
pub fn http_client() -> &'static Client {
    &HTTP_CLIENT
}

/// Truncate `s` to at most `max_bytes` bytes without ever splitting a
/// multi-byte UTF-8 character. Returns a slice ending on the largest char
/// boundary `<= max_bytes`. Never panics, even when `max_bytes` lands inside a
/// CJK (3-byte) character -- the failure mode that crashed the app on Chinese
/// email / memory / tool output.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_shorter_than_limit_is_unchanged() {
        assert_eq!(truncate_bytes("hello", 100), "hello");
    }

    #[test]
    fn cjk_cut_mid_character_never_panics_and_stays_valid() {
        // Each Chinese char is 3 bytes; a 4000-byte cut lands mid-character.
        let s = "審視收購計劃".repeat(1000); // 18000 bytes, boundary mid-char
        let out = truncate_bytes(&s, 4000);
        assert!(out.len() <= 4000);
        assert!(s.is_char_boundary(out.len())); // ended on a real char boundary
        assert!(s.starts_with(out)); // is a genuine prefix
    }

    #[test]
    fn backs_off_to_the_char_boundary_below_the_limit() {
        let s = "abc你好"; // a,b,c = 3 bytes, 你 = 3, 好 = 3 => 9 bytes total
        assert_eq!(truncate_bytes(s, 3), "abc"); // exact boundary after 'c'
        assert_eq!(truncate_bytes(s, 4), "abc"); // byte 4 is inside 你 -> back off
        assert_eq!(truncate_bytes(s, 6), "abc你"); // boundary after 你
    }
}
