//! Persistencia de la sesión de TikTok.
//!
//! Guarda las cookies de una cuenta real para no tener que iniciar sesión en cada
//! arranque. Es material sensible: `sessionid` **es** la cuenta, así que el fichero se
//! escribe con permisos `0600` y nunca se loguea en claro
//! (`docs/06-risks-and-ops.md` §Seguridad de los fixtures).

use std::io;
use std::path::{Path, PathBuf};

use ttl_sign_core::CookieJar;

/// Cookies que merece la pena conservar entre arranques.
///
/// El resto (analítica, tema de la interfaz, banderas de experimentos) se descarta: no
/// hace falta y guardar menos es guardar mejor.
const PERSISTED: &[&str] = &[
    "sessionid",
    "sessionid_ss",
    "sid_tt",
    "sid_guard",
    "uid_tt",
    "uid_tt_ss",
    "tt-target-idc",
    "ttwid",
    "odin_tt",
];

/// La cookie que decide si hay sesión o no.
pub const SESSION_COOKIE: &str = "sessionid";

/// Dónde se guarda por defecto: `$XDG_CONFIG_HOME/ttl-signer/session`, o
/// `~/.config/ttl-signer/session`. Fuera del repositorio, a propósito.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("ttl-signer").join("session"))
}

/// Ruta efectiva: `TTL_SESSION_FILE` si está, si no la de por defecto.
pub fn configured_path() -> Option<PathBuf> {
    std::env::var_os("TTL_SESSION_FILE")
        .map(PathBuf::from)
        .or_else(default_path)
}

/// Guarda las cookies con permisos `0600`.
///
/// Los permisos se ponen **antes** de escribir nada: crear el fichero legible y
/// restringirlo después deja una ventana en la que cualquiera puede leer la sesión.
pub fn save(path: &Path, jar: &CookieJar) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let filtered = filter(jar);
    let contents = filtered.to_cookie_string();

    use std::io::Write;
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Lee las cookies guardadas. `Ok(None)` si el fichero no existe.
pub fn load(path: &Path) -> io::Result<Option<CookieJar>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(CookieJar::parse(contents.trim()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Se queda solo con las cookies de [`PERSISTED`] que tengan valor.
pub fn filter(jar: &CookieJar) -> CookieJar {
    jar.iter()
        .filter(|(name, value)| PERSISTED.contains(name) && !value.is_empty())
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// ¿Estas cookies son una sesión iniciada?
pub fn is_logged_in(jar: &CookieJar) -> bool {
    jar.get(SESSION_COOKIE).is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_useful_cookies() {
        let jar = CookieJar::parse(
            "sessionid=abc; tiktok_webapp_theme=dark; ttwid=xyz; vacia=; odin_tt=o",
        );
        let filtered = filter(&jar);
        assert_eq!(filtered.get("sessionid"), Some("abc"));
        assert_eq!(filtered.get("ttwid"), Some("xyz"));
        assert_eq!(filtered.get("odin_tt"), Some("o"));
        assert_eq!(filtered.get("tiktok_webapp_theme"), None);
        assert_eq!(filtered.get("vacia"), None, "una cookie vacía no es sesión");
    }

    #[test]
    fn detects_a_logged_in_jar() {
        assert!(is_logged_in(&CookieJar::parse("sessionid=abc")));
        assert!(!is_logged_in(&CookieJar::parse("sessionid=")));
        assert!(!is_logged_in(&CookieJar::parse("ttwid=xyz")));
    }

    #[test]
    fn roundtrips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("ttl-session-test-{}", std::process::id()));
        let path = dir.join("session");
        let jar = CookieJar::parse("sessionid=secreto; ttwid=xyz");

        save(&path, &jar).unwrap();
        let loaded = load(&path).unwrap().expect("el fichero existe");
        assert_eq!(loaded.get("sessionid"), Some("secreto"));
        assert!(is_logged_in(&loaded));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "la sesión no puede quedar legible");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let path = std::env::temp_dir().join("ttl-session-que-no-existe");
        std::fs::remove_file(&path).ok();
        assert!(load(&path).unwrap().is_none());
    }
}
