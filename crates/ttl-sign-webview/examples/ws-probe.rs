//! Diagnóstico del WebSocket: enseña **todo** lo que entra y sale por el socket.
//!
//! `live-check` filtra los frames de transporte y solo cuenta los `msg`; cuando no llega
//! nada, eso no distingue "el servidor calla" de "nos está hablando y lo descartamos".
//! Aquí no se filtra nada.
//!
//! Prueba además el guion que espera el servidor: entrada a la sala, silencio, luego un
//! heartbeat de aplicación (`hb`) y un ping de protocolo (el handshake anuncia
//! `ping-interval`).
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example ws-probe -- <usuario en directo>
//! ```

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use ttl_sign_core::proto::PushFrame;
use ttl_sign_core::room::live_page_url;
use ttl_sign_core::{FetchResult, SignOutcome, WsParams};
use ttl_sign_webview::{run, session, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=warn")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("uso: ws-probe <usuario en directo>");
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
        session,
        ..EngineConfig::default()
    };
    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        rt.block_on(async move {
            let lookup = signer.room_lookup(&user).await.expect("lookup");
            let signed = match signer.fetch(&lookup.room_id).await {
                SignOutcome::Ok(s) => s,
                other => {
                    eprintln!("no se pudo firmar: {other:?}");
                    std::process::exit(1);
                }
            };
            let result = FetchResult::decode(&signed.protobuf).expect("protobuf");

            let mut params = WsParams::new(&lookup.room_id);
            params.cursor = result.cursor.clone();
            params.internal_ext = result.internal_ext.clone();
            let preset = signer.preset();
            let uri = params.build_uri(&result.push_server, &result.route_params, &preset);
            let uri = signer.sign_ws_uri(&uri).await.expect("firma del WebSocket");
            println!("host: {}", uri.split('?').next().unwrap_or_default());

            let mut request = uri.as_str().into_client_request().expect("uri");
            request.headers_mut().insert(
                "Cookie",
                HeaderValue::from_str(&signed.cookies.to_cookie_string()).unwrap(),
            );
            request.headers_mut().insert(
                "User-Agent",
                HeaderValue::from_str(&signed.user_agent).unwrap(),
            );
            request.headers_mut().insert(
                "Origin",
                HeaderValue::from_static("https://www.tiktok.com"),
            );

            let (mut ws, response) = tokio_tungstenite::connect_async(request)
                .await
                .expect("handshake");
            println!("handshake {} ", response.status());
            for (k, v) in response.headers() {
                println!("  {k}: {}", v.to_str().unwrap_or("?"));
            }

            let room_id = lookup.room_id.parse().expect("room_id");
            let enter_room = PushFrame::enter_room(room_id);
            println!(
                "[{:>5.1}s] → im_enter_room ({} bytes)",
                0.0,
                enter_room.payload.len()
            );
            ws.send(Message::Binary(enter_room.encode()))
                .await
                .expect("entrada a la sala");

            let t0 = Instant::now();
            let mut deadline = tokio::time::interval(Duration::from_secs(5));
            let mut fase = 0;

            loop {
                tokio::select! {
                    _ = deadline.tick() => {
                        fase += 1;
                        match fase {
                            1 => println!("[{:>5.1}s] escuchando en silencio…", t0.elapsed().as_secs_f32()),
                            2 => {
                                let hb = PushFrame::heartbeat(room_id, 1);
                                println!("[{:>5.1}s] → hb de aplicación ({:02x?})", t0.elapsed().as_secs_f32(), hb.encode());
                                ws.send(Message::Binary(hb.encode())).await.ok();
                            }
                            3 => {
                                println!("[{:>5.1}s] → ping de protocolo", t0.elapsed().as_secs_f32());
                                ws.send(Message::Ping(Vec::new())).await.ok();
                            }
                            6 => {
                                println!("[{:>5.1}s] fin", t0.elapsed().as_secs_f32());
                                break;
                            }
                            _ => {}
                        }
                    }
                    entrada = ws.next() => {
                        let t = t0.elapsed().as_secs_f32();
                        match entrada {
                            None => { println!("[{t:>5.1}s] socket cerrado por el otro lado"); break; }
                            Some(Err(e)) => { println!("[{t:>5.1}s] error: {e}"); break; }
                            Some(Ok(Message::Binary(bytes))) => {
                                match PushFrame::decode(&bytes) {
                                    Ok(f) => println!(
                                        "[{t:>5.1}s] ← binario {} bytes: payload_type={:?} encoding={:?} headers={:?} payload={} bytes",
                                        bytes.len(), f.payload_type, f.payload_encoding, f.headers, f.payload.len()
                                    ),
                                    Err(e) => println!("[{t:>5.1}s] ← binario {} bytes, ilegible: {e}", bytes.len()),
                                }
                            }
                            Some(Ok(otro)) => println!("[{t:>5.1}s] ← {otro:?}"),
                        }
                    }
                }
            }

            signer.shutdown();
            std::process::exit(0);
        })
    })
}
