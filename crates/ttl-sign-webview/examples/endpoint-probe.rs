//! Compara endpoints de TikTok LIVE usando exactamente la misma ruta de firma.
//!
//! Firma cada URL con el SDK de la página y la repite desde Rust, enseñando status y
//! bytes. Sirve para separar dos cosas que se confunden con facilidad: "nuestra forma de
//! repetir la petición está mal" y "ese endpoint ya no responde".
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example endpoint-probe -- usuario
//! ```

use std::time::Duration;

use ttl_sign_core::room::live_page_url;
use ttl_sign_core::{FetchParams, Preset};
use ttl_sign_webview::{run, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("uso: endpoint-probe <usuario en directo>");
        std::process::exit(2);
    });

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        ..EngineConfig::default()
    };
    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        rt.block_on(async move {
            let lookup = signer.room_lookup(&user).await.expect("lookup");
            println!(
                "@{} room_id={} en_directo={}",
                lookup.unique_id,
                lookup.room_id,
                lookup.is_live()
            );

            let preset = signer.preset();
            for (label, url) in candidates(&lookup.room_id, &preset) {
                match signer.sign_url(&url).await {
                    Ok(signed) => {
                        let firmado = signed.url.contains("X-Gnarly=");
                        match signer.replay_raw(&signed).await {
                            Ok((status, body)) => {
                                let text = String::from_utf8_lossy(&body);
                                let head = text.chars().take(90).collect::<String>();
                                println!(
                                    "{label:<22} firmado={firmado} status={status} bytes={} {}",
                                    body.len(),
                                    head.replace('\n', " ")
                                );
                                // ¿Trae lo que hace falta para abrir el WebSocket?
                                let pistas: Vec<&str> = [
                                    "push_server",
                                    "route_params",
                                    "internal_ext",
                                    "cursor",
                                    "wss://",
                                ]
                                .into_iter()
                                .filter(|k| text.contains(k))
                                .collect();
                                if !pistas.is_empty() {
                                    println!("{:<22} → pistas de WebSocket: {pistas:?}", "");
                                    if let Some(i) = text.find("wss://") {
                                        println!(
                                            "{:<22} → {}",
                                            "",
                                            text[i..].chars().take(120).collect::<String>()
                                        );
                                    }
                                }
                            }
                            Err(e) => println!("{label:<22} firmado={firmado} ERROR {e}"),
                        }
                    }
                    Err(e) => println!("{label:<22} no se pudo firmar: {e}"),
                }
            }

            signer.shutdown();
            std::process::exit(0);
        })
    })
}

/// Endpoints a comparar: el del camino crítico y otros que la página sí usa hoy.
fn candidates(room_id: &str, preset: &Preset) -> Vec<(String, String)> {
    let common = format!(
        "aid=1988&app_language={lang}&app_name=tiktok_web&browser_language={blang}\
         &browser_name=Mozilla&browser_online=true&browser_platform={plat}\
         &cookie_enabled=true&device_platform=web&identity=audience&live_id=12\
         &webcast_language={lang}",
        lang = preset.location.language,
        blang = preset.location.browser_language,
        plat = preset.device.browser_platform,
    );

    vec![
        (
            "im/fetch (F1)".into(),
            FetchParams::new(room_id).url(preset),
        ),
        (
            "im/fetch (mínimo)".into(),
            format!(
                "https://webcast.tiktok.com/webcast/im/fetch/?{common}&room_id={room_id}\
                 &resp_content_type=protobuf&cursor=&internal_ext=&sup_ws_ds_opt=1&did_rule=3&fetch_rule=1"
            ),
        ),
        (
            "room/enter".into(),
            format!("https://webcast.tiktok.com/webcast/room/enter/?{common}&room_id={room_id}"),
        ),
        (
            "room/check_alive".into(),
            format!("https://webcast.tiktok.com/webcast/room/check_alive/?{common}&room_ids={room_id}"),
        ),
        (
            "room/info".into(),
            format!("https://webcast.tiktok.com/webcast/room/info/?{common}&room_id={room_id}"),
        ),
    ]
}
