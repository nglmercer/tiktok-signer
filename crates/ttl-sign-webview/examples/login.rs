//! Log in to TikTok manually and save the session for the other tools.
//!
//! Opens a real login window, waits for completion with a deadline, and saves session
//! cookies in a `0600` file. `live-check` and the server then load it automatically.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example login
//! cargo run -p ttl-sign-webview --example login -- --timeout 600
//! cargo run -p ttl-sign-webview --example login -- --file /ruta/a/sesion
//! ```
//!
//! Login is required because the anonymous flow no longer works: `/webcast/im/fetch/`
//! returns 200 with an empty body and `/webcast/room/enter/` says `User doesn't login`
//! (`docs/06-risks-and-ops.md` §Decisiones abiertas).
//!
//! **The saved data represents the account.** Anyone holding the cookie is you to TikTok.
//! It lives outside the repository, is readable only by your user, and is deleted with `--logout`.

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
        .ok_or("cannot determine where to save session: set TTL_SESSION_FILE")?;
    let mut logout = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--timeout" => {
                let value = args.next().ok_or("--timeout requires seconds")?;
                let secs: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid timeout: {value}"))?;
                timeout = Duration::from_secs(secs);
            }
            "--file" => path = PathBuf::from(args.next().ok_or("--file requires a path")?),
            "--logout" => logout = true,
            "--help" | "-h" => {
                println!(
                    "usage: login [--timeout <seconds>] [--file <path>] [--logout]\n\
                     \n  --timeout  login deadline in seconds (default 300)\
                     \n  --file     save path (default $XDG_CONFIG_HOME/ttl-signer/session)\
                     \n  --logout   delete saved session and exit"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
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
            Ok(()) => println!("session deleted: {}", args.path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("no saved session at {}", args.path.display())
            }
            Err(e) => {
                eprintln!("could not delete {}: {e}", args.path.display());
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    // If a session exists, warn before opening anything: repeating login unnecessarily is
    // exposing the account without a reason.
    if let Ok(Some(jar)) = session::load(&args.path) {
        if session::is_logged_in(&jar) {
            println!(
                "A saved session already exists at {} ({}).\n\
                 To replace it, log in through the window that opens;\n\
                 to delete it, use --logout.\n",
                args.path.display(),
                jar // Display redacts values.
            );
        }
    }

    println!(
        "A TikTok login window will open.\n\
         You have {} s to log in; the window closes when login is detected.\n",
        args.timeout.as_secs()
    );

    run(EngineConfig::for_login(), move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        let code = rt.block_on(async move {
            // One notice every 30 s: enough to show that it is alive without filling the
            // terminal.
            let mut next_notice = args.timeout.as_secs();
            let resultado = signer
                .wait_for_login(args.timeout, |restante| {
                    let remaining = restante.as_secs();
                    if remaining <= next_notice {
                        println!("  waiting… {remaining} s remaining");
                        next_notice = remaining.saturating_sub(30);
                    }
                })
                .await;

            let code = match resultado {
                Ok(jar) => match session::save(&args.path, &jar) {
                    Ok(()) => {
                        println!("\nLogged in and saved session to {}", args.path.display());
                        println!("cookies: {jar}"); // redacted
                        println!(
                            "\nYou can now use it:\n\
                             \n    cargo run -p ttl-sign-webview --example live-check\
                             \n    cargo run -p ttl-sign-server\n\
                             \nTo delete it: cargo run -p ttl-sign-webview --example login -- --logout"
                        );
                        0
                    }
                    Err(e) => {
                        eprintln!("\nlogin succeeded but session could not be saved: {e}");
                        1
                    }
                },
                Err(e) => {
                    eprintln!("\nno session detected: {e}");
                    eprintln!(
                        "If you need more time: --timeout <seconds>. \
                         Nothing was saved."
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
