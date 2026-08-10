//! `CookieJar` mínimo, en formato cookie-string.
//!
//! Es el formato de la cabecera `X-Set-TT-Cookie` que exige el cliente Python de
//! referencia (`docs/00-research.md` §4) y también el del header `Cookie` del WebSocket.
//! Si falta, el cliente aborta con `EMPTY_COOKIES` antes de intentar conectar.

use std::fmt;

/// Cookies de una sesión, en orden de inserción.
///
/// `Display` **redacta** los valores (`docs/06-risks-and-ops.md` §Seguridad de los
/// fixtures): para obtener la cadena real hay que llamar a [`CookieJar::to_cookie_string`],
/// que es explícita sobre lo que hace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieJar {
    entries: Vec<(String, String)>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parsea `k=v; k=v`. Ignora los segmentos vacíos o sin `=`.
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

    /// Inserta o reemplaza, conservando la posición original de la clave.
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

    /// Fusiona `other` encima de este jar.
    pub fn merge(&mut self, other: &CookieJar) {
        for (k, v) in other.iter() {
            self.set(k, v);
        }
    }

    /// La cadena real, para `X-Set-TT-Cookie` y para el header `Cookie` del WS.
    ///
    /// Contiene secretos de sesión: no meterla en logs (usar [`CookieJar::redacted`]).
    pub fn to_cookie_string(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Versión para logs: solo los 8 primeros caracteres de cada valor.
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

    /// ¿Están las cookies que el WebSocket necesita?
    ///
    /// `msToken` es el token anti-replay y `tt-target-idc` fija el datacenter; sin ellas
    /// el handshake del WS se rechaza aunque la firma fuese válida.
    pub fn has_required_for_ws(&self) -> bool {
        self.get("msToken").is_some_and(|v| !v.is_empty())
    }
}

impl fmt::Display for CookieJar {
    /// Redactado a propósito: ver [`CookieJar::to_cookie_string`].
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
            "el valor no está redactado: {shown}"
        );
    }

    #[test]
    fn detects_missing_mstoken() {
        assert!(!CookieJar::parse("ttwid=x").has_required_for_ws());
        assert!(CookieJar::parse("msToken=x").has_required_for_ws());
        assert!(!CookieJar::parse("msToken=").has_required_for_ws());
    }
}
