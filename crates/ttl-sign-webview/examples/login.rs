//! Inicia sesión en TikTok a mano y guarda la sesión para el resto de herramientas.
//!
//! Abre una ventana real con la página de login, espera a que termines (con un plazo) y
//! guarda las cookies de sesión en un fichero con permisos `0600`. A partir de ahí,
//! `live-check` y el servidor la cogen solos.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example login
//! cargo run -p ttl-sign-webview --example login -- --timeout 600
//! cargo run -p ttl-sign-webview --example login -- --file /ruta/a/sesion
//! ```
//!
//! Hace falta porque el flujo anónimo dejó de funcionar: `/webcast/im/fetch/` responde
//! 200 con cuerpo vacío y `/webcast/room/enter/` dice `User doesn't login`
//! (`docs/06-risks-and-ops.md` §Decisiones abiertas).
//!
//! **Lo que se guarda es la cuenta.** Con esa cookie, quien la tenga es tú para TikTok.
//! Vive fuera del repositorio, solo la lee tu usuario, y se borra con `--logout`.

use std::path::PathBuf;
use std::time::Duration;

use ttl_sign_webview::{run, session, EngineConfig, Signer};

struct Args {
    timeout: Duration,
    path: PathBuf,
    logout: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut timeout = Duration::from_secs(300);
    let mut path = session::configured_path()
        .ok_or("no sé dónde guardar la sesión: define TTL_SESSION_FILE")?;
    let mut logout = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--timeout" => {
                let value = args.next().ok_or("--timeout necesita segundos")?;
                let secs: u64 = value
                    .parse()
                    .map_err(|_| format!("plazo inválido: {value}"))?;
                timeout = Duration::from_secs(secs);
            }
            "--file" => path = PathBuf::from(args.next().ok_or("--file necesita una ruta")?),
            "--logout" => logout = true,
            "--help" | "-h" => {
                println!(
                    "uso: login [--timeout <segundos>] [--file <ruta>] [--logout]\n\
                     \n  --timeout  cuánto se espera a que inicies sesión (por defecto 300)\
                     \n  --file     dónde se guarda (por defecto $XDG_CONFIG_HOME/ttl-signer/session)\
                     \n  --logout   borra la sesión guardada y sale"
                );
                std::process::exit(0);
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(Args {
        timeout,
        path,
        logout,
    })
}

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_webview=warn".into()),
        )
        .init();

    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    if args.logout {
        match std::fs::remove_file(&args.path) {
            Ok(()) => println!("sesión borrada: {}", args.path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("no había sesión guardada en {}", args.path.display())
            }
            Err(e) => {
                eprintln!("no se pudo borrar {}: {e}", args.path.display());
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    // Si ya hay sesión, se avisa antes de abrir nada: repetir el login por costumbre es
    // exponer la cuenta sin motivo.
    if let Ok(Some(jar)) = session::load(&args.path) {
        if session::is_logged_in(&jar) {
            println!(
                "Ya hay una sesión guardada en {} ({}).\n\
                 Para reemplazarla, inicia sesión igualmente en la ventana que se abre;\n\
                 para borrarla, usa --logout.\n",
                args.path.display(),
                jar // Display redacta los valores.
            );
        }
    }

    println!(
        "Se abrirá una ventana con la página de login de TikTok.\n\
         Tienes {} s para iniciar sesión; la ventana se cierra sola al detectarlo.\n",
        args.timeout.as_secs()
    );

    run(EngineConfig::for_login(), move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        let code = rt.block_on(async move {
            // Un aviso cada 30 s: suficiente para saber que sigue vivo, poco suficiente
            // para llenar la terminal.
            let mut proximo_aviso = args.timeout.as_secs();
            let resultado = signer
                .wait_for_login(args.timeout, |restante| {
                    let quedan = restante.as_secs();
                    if quedan <= proximo_aviso {
                        println!("  esperando… {quedan} s restantes");
                        proximo_aviso = quedan.saturating_sub(30);
                    }
                })
                .await;

            let code = match resultado {
                Ok(jar) => match session::save(&args.path, &jar) {
                    Ok(()) => {
                        println!("\nSesión iniciada y guardada en {}", args.path.display());
                        println!("cookies: {jar}"); // redactadas
                        println!(
                            "\nYa puedes usarla:\n\
                             \n    cargo run -p ttl-sign-webview --example live-check\
                             \n    cargo run -p ttl-sign-server\n\
                             \nPara borrarla: cargo run -p ttl-sign-webview --example login -- --logout"
                        );
                        0
                    }
                    Err(e) => {
                        eprintln!("\nse inició sesión pero no se pudo guardar: {e}");
                        1
                    }
                },
                Err(e) => {
                    eprintln!("\nno se detectó ninguna sesión: {e}");
                    eprintln!(
                        "Si necesitas más tiempo: --timeout <segundos>. \
                         Nada se ha guardado."
                    );
                    1
                }
            };
            signer.shutdown();
            code
        });
        std::process::exit(code);
    })
}
