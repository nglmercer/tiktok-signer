//! Resultado de una firma.
//!
//! Regla que viene de `docs/06-risks-and-ops.md` §2 y que ordena todo este módulo:
//! **rechazo y error de transporte nunca comparten variante.** Un 200 con cuerpo vacío
//! no es un fallo transitorio, es "TikTok te ha rechazado", y reintentarlo empeora las
//! cosas. Si ambos casos cayesen en el mismo tipo, el primer `retry` genérico que
//! alguien añada convertiría una detección en un bucle.

use crate::cookie::CookieJar;

/// Firma completada con éxito: los bytes protobuf **tal cual** los devolvió TikTok, más
/// el contexto necesario para abrir el WebSocket con la misma identidad.
#[derive(Debug, Clone)]
pub struct SignedFetch {
    /// Cuerpo de la respuesta, sin envolver ni transformar.
    pub protobuf: Vec<u8>,
    /// Cookies de la sesión que firmó. Van en `X-Set-TT-Cookie` y en el header `Cookie`.
    pub cookies: CookieJar,
    /// UA exacto usado al firmar. El WS tiene que usar este mismo.
    pub user_agent: String,
    /// URL final de la petición, ya con `X-Bogus`/`X-Gnarly` añadidos por el SDK.
    /// Es lo que habilita el Plan B (`docs/01-architecture.md` §D2); en Plan A es
    /// puramente informativo.
    pub signed_url: String,
}

/// Por qué TikTok rechazó, en los términos en que se puede distinguir desde fuera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// 200 con cuerpo vacío.
    EmptyBody,
    /// 200 con protobuf válido pero `push_server` vacío: rechazo silencioso.
    EmptyPushServer,
    /// Código HTTP distinto de 200.
    HttpStatus(u16),
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBody => write!(f, "cuerpo vacío (rechazo silencioso)"),
            Self::EmptyPushServer => write!(f, "push_server vacío (rechazo silencioso)"),
            Self::HttpStatus(code) => write!(f, "TikTok respondió con HTTP {code}"),
        }
    }
}

/// Fallos que **sí** pueden ser transitorios o de configuración, nunca detección.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    /// `window.byted_acrawler` no apareció dentro del plazo
    /// (`docs/04-spec-webview-bridge.md` §Readiness gate).
    #[error("el SDK de TikTok no estuvo listo a tiempo")]
    SdkNotReady,

    /// El pool no tiene ninguna instancia disponible.
    #[error("no hay ninguna instancia de webview disponible")]
    NoInstanceAvailable,

    /// La página respondió con un error de JS.
    #[error("fallo dentro de la página: {0}")]
    Bridge(String),

    /// El motor de webview se cerró o dejó de responder.
    #[error("el motor de webview no responde: {0}")]
    EngineGone(String),

    /// Nadie inició sesión dentro del plazo del flujo de login.
    #[error("no se inició sesión en el plazo ({0} s)")]
    LoginTimeout(u64),

    /// La firma tardó más de lo permitido. Es relevante porque el resultado caduca
    /// a los ~30 s: llegar tarde equivale a no llegar.
    #[error("la firma superó el tiempo máximo ({0} ms)")]
    Timeout(u64),

    /// Error de red al hablar con TikTok.
    #[error("error de transporte: {0}")]
    Transport(String),

    /// La respuesta no se pudo decodificar.
    #[error("respuesta ilegible: {0}")]
    Decode(String),
}

/// Los tres finales posibles de una firma. No hay un cuarto.
#[derive(Debug)]
pub enum SignOutcome {
    /// Firma válida.
    Ok(SignedFetch),
    /// TikTok nos rechazó. **No reintentar**: no es transitorio.
    Rejected(RejectReason),
    /// Algo se rompió por el camino. Puede tener sentido reintentar, con criterio.
    Transport(SignError),
}

impl SignOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// ¿Es un rechazo? Lo usa el watchdog para contar rechazos consecutivos
    /// (`docs/06-risks-and-ops.md` §Señales a instrumentar).
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected(_))
    }

    /// ¿Tiene sentido reintentar esto? `false` para todo rechazo, siempre.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    pub fn ok(self) -> Option<SignedFetch> {
        match self {
            Self::Ok(f) => Some(f),
            _ => None,
        }
    }
}

impl From<SignError> for SignOutcome {
    fn from(e: SignError) -> Self {
        Self::Transport(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejection_is_never_retryable() {
        for reason in [
            RejectReason::EmptyBody,
            RejectReason::EmptyPushServer,
            RejectReason::HttpStatus(403),
        ] {
            let outcome = SignOutcome::Rejected(reason);
            assert!(outcome.is_rejected());
            assert!(
                !outcome.is_retryable(),
                "un rechazo nunca se reintenta: es detección, no un transitorio"
            );
        }
    }

    #[test]
    fn transport_errors_are_retryable() {
        let outcome = SignOutcome::Transport(SignError::Transport("connection reset".into()));
        assert!(outcome.is_retryable());
        assert!(!outcome.is_rejected());
    }
}
