//! Mira lo que hace el reproductor real, sin pedirle nada a la página.
//!
//! Carga `https://www.tiktok.com/@<usuario>/live` con la sesión guardada y se limita a
//! grabar: qué peticiones `webcast` salen, si su respuesta se puede leer desde la página,
//! y qué URI de WebSocket abre el reproductor. Es la verdad de referencia contra la que
//! comparar lo que construimos nosotros.
//!
//! No firma nada por su cuenta: una sola carga de página, que es lo que haría cualquiera
//! viendo un directo.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example observe -- <usuario en directo>
//! ```

use std::time::Duration;

use ttl_sign_core::room::live_page_url;
use ttl_sign_core::FetchResult;
use ttl_sign_webview::{run, session, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("uso: observe <usuario en directo>");
        std::process::exit(2);
    });

    let session = session::configured_path()
        .and_then(|p| session::load(&p).ok().flatten())
        .unwrap_or_default();
    if !session::is_logged_in(&session) {
        eprintln!("hace falta sesión: cargo run -p ttl-sign-webview --example login");
        std::process::exit(1);
    }

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        // Esta herramienta necesita ver la URI que abre el reproductor real y no conecta
        // un socket Rust propio.
        block_page_websockets: false,
        session,
        ..EngineConfig::default()
    };

    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        rt.block_on(async move {
            // Lo primero, si la sesión llegó a la página: si no, todo lo demás se explica
            // solo y no hace falta seguir mirando.
            match signer.cookies().await {
                Ok(jar) => {
                    let mut names: Vec<&str> = jar.iter().map(|(k, _)| k).collect();
                    names.sort_unstable();
                    println!("cookies del webview: {}", names.join(" "));
                    println!(
                        "sesión en el navegador: {}",
                        if session::is_logged_in(&jar) {
                            "sí"
                        } else {
                            "NO — la página navegará como anónima"
                        }
                    );
                }
                Err(e) => println!("no se pudieron leer las cookies: {e}"),
            }
            match signer
                .eval("String(!!(document.cookie||'').match(/sid_guard|sessionid/))")
                .await
            {
                Ok(v) => println!("la página se ve a sí misma con sesión: {v}"),
                Err(e) => println!("no se pudo preguntar a la página: {e}"),
            }

            // El reproductor tarda en arrancar. Se mira varias veces en vez de una:
            // interesa tanto qué pide como en qué orden.
            for t in [8u64, 8, 14] {
                tokio::time::sleep(Duration::from_secs(t)).await;
                println!("\n=== t+{t}s ===");
                dump(&signer).await;
            }
            signer.shutdown();
            std::process::exit(0);
        })
    })
}

async fn dump(signer: &Signer) {
    match signer.captures().await {
        Ok(captures) if captures.is_empty() => println!("  (la página no ha pedido nada de im/)"),
        Ok(captures) => {
            for c in &captures {
                println!(
                    "  {} vía {} status={} tipo={:?} {} bytes{}",
                    c.endpoint(),
                    c.via,
                    c.status,
                    c.response_type,
                    c.bytes,
                    c.error
                        .as_ref()
                        .map(|e| format!("  ERROR {e}"))
                        .unwrap_or_default()
                );
                if !c.text.is_empty() {
                    println!("    cuerpo: {}", c.text.replace('\n', " "));
                }
                // Los parámetros son lo que se compara contra los nuestros.
                if let Some(query) = c.url.split_once('?').map(|(_, q)| q) {
                    println!("    params: {}", resumir(query));
                }
                let body = c.decoded();
                if !body.is_empty() {
                    match FetchResult::decode(&body) {
                        Ok(r) => println!(
                            "    protobuf: push_server={} route_params={:?} cursor={} internal_ext={} chars",
                            r.push_server,
                            r.route_params.iter().map(|(k, _)| k).collect::<Vec<_>>(),
                            r.cursor,
                            r.internal_ext.len()
                        ),
                        Err(e) => println!("    protobuf ilegible: {e}"),
                    }
                }
            }
        }
        Err(e) => println!("  no se pudieron leer las capturas: {e}"),
    }

    match signer.page_ws_urls().await {
        Ok(urls) if urls.is_empty() => println!("  (la página no ha abierto ningún WebSocket)"),
        Ok(urls) => {
            for url in &urls {
                let (base, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
                println!("  WS {base}");
                println!("    params: {}", resumir(query));
            }
        }
        Err(e) => println!("  no se pudo leer la lista de WebSockets: {e}"),
    }
}

/// `k=v` separados por espacios, con los valores largos o sensibles recortados: las
/// firmas y los tokens no aportan nada leídos enteros y sí ocupan la pantalla.
fn resumir(query: &str) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if v.len() > 24 || matches!(k, "msToken" | "X-Gnarly" | "X-Dynosaur" | "X-Bogus") {
                format!("{k}=<{} chars>", v.len())
            } else {
                format!("{k}={v}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
