//! Minimal `CookieJar` in cookie-string format.
//!
//! This is the format required by the reference Python client's `X-Set-TT-Cookie` header
//! and by the WebSocket `Cookie` header. If it is missing, the client aborts with
//! `EMPTY_COOKIES` before connecting.

use std::fmt;

/// Session cookies in insertion order.
///
/// `Display` **redacts** values: call [`CookieJar::to_cookie_string`] explicitly when the
/// raw cookie string is required.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieJar {
    entries: Vec<(String, String)>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `k=v; k=v`. Empty segments and segments without `=` are ignored.
    pub fn parse(raw: &str) -> Self {
        let mut jar = Self::new();
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                let k = k.trim();
                if !k.is_empty() {
                    jar.set(k, v.trim());
                }
            }
        }
        jar
    }

    /// Insert or replace while preserving the key's original position.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let name = name.into();
        let value = value.into();
        match self.entries.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((name, value)),
        }
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|(k, _)| k != name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Merge `other` over this jar.
    pub fn merge(&mut self, other: &CookieJar) {
        for (k, v) in other.iter() {
            self.set(k, v);
        }
    }

    /// Raw string for `X-Set-TT-Cookie` and the WebSocket `Cookie` header.
    ///
    /// Contains session secrets: never log it (use [`CookieJar::redacted`]).
    pub fn to_cookie_string(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Log-safe version: only the first eight characters of each value.
    pub fn redacted(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| {
                let head: String = v.chars().take(8).collect();
                if v.chars().count() > 8 {
                    format!("{k}={head}…")
                } else {
                    format!("{k}={head}")
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Does the jar contain the cookies required by the WebSocket?
    ///
    /// `msToken` is the anti-replay token and `tt-target-idc` selects the datacenter; without them
    /// the WebSocket handshake is rejected even when the signature is valid.
    pub fn has_required_for_ws(&self) -> bool {
        self.get("msToken").is_some_and(|v| !v.is_empty())
    }
}

impl fmt::Display for CookieJar {
    /// Intentionally redacted; see [`CookieJar::to_cookie_string`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl FromIterator<(String, String)> for CookieJar {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut jar = Self::new();
        for (k, v) in iter {
            jar.set(k, v);
        }
        jar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_reserializes() {
        let jar = CookieJar::parse("msToken=abc; tt-target-idc=useast1a; ttwid=xyz");
        assert_eq!(jar.len(), 3);
        assert_eq!(jar.get("tt-target-idc"), Some("useast1a"));
        assert_eq!(
            jar.to_cookie_string(),
            "msToken=abc; tt-target-idc=useast1a; ttwid=xyz"
        );
    }

    #[test]
    fn tolerates_junk_segments() {
        let jar = CookieJar::parse("; ; a=1;;  b=2 ; novalue ; =3");
        assert_eq!(jar.to_cookie_string(), "a=1; b=2");
    }

    #[test]
    fn keeps_position_when_overwriting() {
        let mut jar = CookieJar::parse("a=1; b=2");
        jar.set("a", "9");
        assert_eq!(jar.to_cookie_string(), "a=9; b=2");
    }

    #[test]
    fn display_redacts_secrets() {
        let jar = CookieJar::parse("sessionid=0123456789abcdef");
        let shown = jar.to_string();
        assert!(shown.contains("01234567"), "{shown}");
        assert!(
            !shown.contains("89abcdef"),
            "value was not redacted: {shown}"
        );
    }

    #[test]
    fn detects_missing_mstoken() {
        assert!(!CookieJar::parse("ttwid=x").has_required_for_ws());
        assert!(CookieJar::parse("msToken=x").has_required_for_ws());
        assert!(!CookieJar::parse("msToken=").has_required_for_ws());
    }
}
