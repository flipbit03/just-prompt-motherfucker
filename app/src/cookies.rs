//! Two CSRF defences. There are no sessions.

use axum::http::{HeaderMap, header};

/// Carries the OAuth `state` plus the intent, for the length of one round trip
/// to GitHub.
pub const STATE: &str = "jpmf_state";

/// Set on every render; its twin is a hidden field in the forms.
pub const CSRF: &str = "jpmf_csrf";

/// Ten minutes: the same ballpark as GitHub's own code expiry.
pub const STATE_MAX_AGE: i64 = 600;
pub const CSRF_MAX_AGE: i64 = 60 * 60 * 24;

pub fn get(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let pair = pair.trim();
        let rest = pair.strip_prefix(name)?.strip_prefix('=')?;
        Some(rest.to_string())
    })
}

/// `SameSite=Lax`, never `Strict`: the callback is a cross-site top-level
/// navigation from github.com, and `Strict` withholds cookies on exactly that.
pub fn set(name: &str, value: &str, max_age: i64, secure: bool) -> String {
    let mut c = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    if secure {
        c.push_str("; Secure");
    }
    c
}

pub fn clear(name: &str, secure: bool) -> String {
    set(name, "", 0, secure)
}

/// 32 bytes of OS randomness, hex encoded.
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random number generator");
    bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Carried through GitHub in the state cookie. Not tamper-proof, and does not
/// need to be: editing it only affects your own signature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Sign,
    Unsign,
}

impl Intent {
    pub fn as_str(self) -> &'static str {
        match self {
            Intent::Sign => "sign",
            Intent::Unsign => "unsign",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sign" => Some(Intent::Sign),
            "unsign" => Some(Intent::Unsign),
            _ => None,
        }
    }
}

/// `<random>.<intent>`, stored in both the cookie and the URL and compared
/// whole.
pub fn state_value(intent: Intent) -> String {
    format!("{}.{}", token(), intent.as_str())
}

pub fn state_intent(state: &str) -> Option<Intent> {
    Intent::parse(state.split('.').nth(1)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(cookie: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        h
    }

    #[test]
    fn reads_one_cookie_among_several() {
        let h = headers("other=1; jpmf_csrf=abc123; last=2");
        assert_eq!(get(&h, CSRF).as_deref(), Some("abc123"));
        assert_eq!(get(&h, STATE), None);
    }

    #[test]
    fn does_not_match_a_longer_name_with_the_same_prefix() {
        // "jpmf_state_other" starts with "jpmf_state" but is a different cookie.
        let h = headers("jpmf_state_other=nope");
        assert_eq!(get(&h, STATE), None);
    }

    #[test]
    fn no_cookie_header_is_not_an_error() {
        assert_eq!(get(&HeaderMap::new(), CSRF), None);
    }

    #[test]
    fn attributes_are_lax_and_conditionally_secure() {
        let insecure = set(CSRF, "v", 600, false);
        assert!(insecure.contains("SameSite=Lax"));
        assert!(insecure.contains("HttpOnly"));
        assert!(!insecure.contains("Secure"));
        assert!(set(CSRF, "v", 600, true).contains("; Secure"));
    }

    #[test]
    fn clearing_expires_immediately() {
        assert!(clear(STATE, false).contains("Max-Age=0"));
    }

    #[test]
    fn tokens_are_64_hex_chars_and_do_not_repeat() {
        let a = token();
        let b = token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn state_carries_the_intent_round_trip() {
        let s = state_value(Intent::Unsign);
        assert_eq!(state_intent(&s), Some(Intent::Unsign));
        assert_eq!(state_intent(&state_value(Intent::Sign)), Some(Intent::Sign));
        assert_eq!(state_intent("garbage"), None);
        assert_eq!(state_intent("abc.nonsense"), None);
    }
}
