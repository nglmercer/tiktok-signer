//! Presets: única fuente de verdad para UA + parámetros de navegador.
//!
//! La causa nº1 de rechazo es que el `User-Agent` no cuadre con `browser_name` /
//! `browser_version` (ver `docs/00-research.md` §3). Por eso aquí **no existe** una API
//! para fijar un UA suelto: el UA se *deriva* del preset y no se puede desincronizar.
//!
//! La relación es la que usan los clientes de referencia:
//!
//! ```text
//! user_agent == format!("{browser_name}/{browser_version}")
//! ```
//!
//! es decir `browser_name = "Mozilla"` y `browser_version = "5.0 (Windows NT 10.0; ...)"`.

use std::fmt;

/// Navegador + sistema operativo. Genera el UA y los params que lo describen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePreset {
    /// `browser_name` de la query. En la práctica, siempre `Mozilla`.
    pub browser_name: String,
    /// `browser_version` de la query: el UA **sin** el prefijo `Mozilla/`.
    pub browser_version: String,
    /// `browser_platform` de la query (`Win32`, `Linux x86_64`, `MacIntel`).
    pub browser_platform: String,
    /// `os` de la query (`windows`, `linux`, `mac`).
    pub os: String,
}

impl DevicePreset {
    /// Chrome estable sobre Windows 10/11. Preset por defecto.
    pub fn chrome_windows() -> Self {
        Self {
            browser_name: "Mozilla".into(),
            browser_version: "5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
            browser_platform: "Win32".into(),
            os: "windows".into(),
        }
    }

    /// Chrome estable sobre Linux.
    pub fn chrome_linux() -> Self {
        Self {
            browser_name: "Mozilla".into(),
            browser_version: "5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
            browser_platform: "Linux x86_64".into(),
            os: "linux".into(),
        }
    }

    /// Chrome estable sobre macOS.
    pub fn chrome_macos() -> Self {
        Self {
            browser_name: "Mozilla".into(),
            browser_version: "5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
            browser_platform: "MacIntel".into(),
            os: "mac".into(),
        }
    }

    /// El `User-Agent` que corresponde a este preset. **La única forma de obtener un UA.**
    pub fn user_agent(&self) -> String {
        format!("{}/{}", self.browser_name, self.browser_version)
    }

    /// Todos los presets conocidos. Usado por los tests de coherencia.
    pub fn all() -> Vec<Self> {
        vec![
            Self::chrome_windows(),
            Self::chrome_linux(),
            Self::chrome_macos(),
        ]
    }
}

impl Default for DevicePreset {
    fn default() -> Self {
        Self::chrome_windows()
    }
}

/// Idioma, zona horaria y región. Todos los campos han de ser coherentes entre sí.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationPreset {
    /// `app_language` / `webcast_language`, p. ej. `en`.
    pub language: String,
    /// `browser_language`, p. ej. `en-US`.
    pub browser_language: String,
    /// `tz_name` en formato IANA, p. ej. `America/New_York`.
    pub tz_name: String,
    /// `region` / `priority_region`, p. ej. `US`.
    pub region: String,
}

impl LocationPreset {
    /// Costa este de EE. UU., inglés.
    pub fn us_east() -> Self {
        Self {
            language: "en".into(),
            browser_language: "en-US".into(),
            tz_name: "America/New_York".into(),
            region: "US".into(),
        }
    }

    /// Europa occidental, español.
    pub fn es() -> Self {
        Self {
            language: "es".into(),
            browser_language: "es-ES".into(),
            tz_name: "Europe/Madrid".into(),
            region: "ES".into(),
        }
    }

    pub fn all() -> Vec<Self> {
        vec![Self::us_east(), Self::es()]
    }
}

impl Default for LocationPreset {
    fn default() -> Self {
        Self::us_east()
    }
}

/// Resolución de pantalla declarada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPreset {
    pub width: u32,
    pub height: u32,
}

impl ScreenPreset {
    pub const FHD: Self = Self {
        width: 1920,
        height: 1080,
    };
    pub const QHD: Self = Self {
        width: 2560,
        height: 1440,
    };
}

impl Default for ScreenPreset {
    fn default() -> Self {
        Self::FHD
    }
}

/// El trío completo. Es lo que se pasa a [`crate::FetchParams`] y [`crate::WsParams`];
/// no existe camino para construir una query con un preset y firmar con otro.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preset {
    pub device: DevicePreset,
    pub location: LocationPreset,
    pub screen: ScreenPreset,
}

impl Preset {
    pub fn new(device: DevicePreset, location: LocationPreset, screen: ScreenPreset) -> Self {
        Self {
            device,
            location,
            screen,
        }
    }

    /// Atajo: el UA de este preset.
    pub fn user_agent(&self) -> String {
        self.device.user_agent()
    }

    /// Todas las combinaciones conocidas, para tests de coherencia.
    pub fn all() -> Vec<Self> {
        let mut out = Vec::new();
        for device in DevicePreset::all() {
            for location in LocationPreset::all() {
                out.push(Self::new(device.clone(), location, ScreenPreset::FHD));
            }
        }
        out
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.device.os, self.location.region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El test que exige `docs/06-risks-and-ops.md` §1: UA y params no se pueden separar.
    #[test]
    fn ua_is_derived_from_browser_params() {
        for d in DevicePreset::all() {
            let ua = d.user_agent();
            assert!(
                ua.starts_with(&format!("{}/", d.browser_name)),
                "el UA no empieza por browser_name: {ua}"
            );
            assert!(
                ua.ends_with(&d.browser_version),
                "el UA no termina en browser_version: {ua}"
            );
        }
    }

    #[test]
    fn platform_matches_os() {
        for d in DevicePreset::all() {
            let ua = d.user_agent();
            let marker = match d.os.as_str() {
                "windows" => "Windows NT",
                "linux" => "Linux",
                "mac" => "Mac OS X",
                other => panic!("os desconocido: {other}"),
            };
            assert!(ua.contains(marker), "UA {ua} no menciona {marker}");
        }
    }

    #[test]
    fn browser_language_starts_with_language() {
        for l in LocationPreset::all() {
            assert!(
                l.browser_language.starts_with(&l.language),
                "{} no es un dialecto de {}",
                l.browser_language,
                l.language
            );
        }
    }
}
