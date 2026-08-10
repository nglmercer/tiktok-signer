//! Verificación contra un canal real, de principio a fin.
//!
//! Recorre el flujo entero de `docs/00-research.md` §1 y dice en qué paso se rompe:
//!
//! ```text
//! 1. descubrir quién está en directo   (DOM renderizado de /live, sin firma)
//! 2. unique_id → room_id + estado      (lookup JSON, sin firma)
//! 3. /webcast/im/fetch/                 ← el único punto firmado
//! 4. abrir el WebSocket y recibir frames
//! ```
//!
//! Es el criterio de aceptación de F2 (y, si llega a los frames, también el de F4).
//!
//! ```sh
//! # descubre el canal solo
//! cargo run -p ttl-sign-webview --example live-check
//!
//! # o fuerza uno concreto, que es más fiable si sabes que está emitiendo
//! cargo run -p ttl-sign-webview --example live-check -- usuario
//! ```
//!
//! Necesita display (X11/Wayland). Sin él: `xvfb-run -a cargo run …`.

use std::time::{Duration, Instant};

use ttl_live_ws::{ConnectConfig, LiveConnection};
use ttl_sign_core::{FetchResult, SignOutcome, WsParams};
use ttl_sign_webview::{run, session, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "live_check=info,ttl_sign_webview=info,ttl_live_ws=debug".into()
            }),
        )
        .init();

    let requested_user = std::env::args().nth(1);
    // Hoy el flujo no funciona en anónimo: ver el paso 3.
    let (config, origen) = configurar_sesion();
    let autenticado = config.is_authenticated();
    println!("sesión: {origen}");

    run(config, move |signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        let code = match rt.block_on(check(signer.clone(), requested_user, autenticado)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("\nFALLA: {e}");
                1
            }
        };
        signer.shutdown();
        std::process::exit(code);
    })
}

/// `TTL_SESSION_ID` manda; si no, la sesión que dejó el ejemplo `login`.
fn configurar_sesion() -> (EngineConfig, String) {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return (
                EngineConfig::default().with_session_id(id),
                "autenticada (TTL_SESSION_ID)".into(),
            );
        }
    }
    if let Some(path) = session::configured_path() {
        if let Ok(Some(jar)) = session::load(&path) {
            if session::is_logged_in(&jar) {
                let origen = format!("autenticada ({})", path.display());
                return (
                    EngineConfig {
                        session: jar,
                        ..EngineConfig::default()
                    },
                    origen,
                );
            }
        }
    }
    (
        EngineConfig::default(),
        "anónima — inicia sesión con: cargo run -p ttl-sign-webview --example login".into(),
    )
}

async fn check(
    signer: Signer,
    requested_user: Option<String>,
    autenticado: bool,
) -> Result<(), String> {
    // --- 1. ¿Quién está en directo? --------------------------------------------------
    let user = match requested_user {
        Some(user) => {
            println!("[1/4] canal indicado a mano: @{user}");
            user
        }
        None => {
            println!("[1/4] esperando a que /live pinte los canales…");
            let channels = poll_channels(&signer).await?;
            for channel in channels.iter().take(10) {
                println!(
                    "      @{}{}",
                    channel.unique_id,
                    if channel.room_id.is_empty() {
                        String::new()
                    } else {
                        format!("  room_id={}", channel.room_id)
                    }
                );
            }
            let first = channels
                .first()
                .ok_or("la página /live no listó ningún canal (¿cargó del todo?)")?;
            println!("      elegido: @{}", first.unique_id);
            first.unique_id.clone()
        }
    };

    // --- 2. unique_id → room_id ------------------------------------------------------
    println!("\n[2/4] resolviendo @{user} → room_id…");
    let lookup = signer
        .room_lookup(&user)
        .await
        .map_err(|e| format!("el lookup falló: {e}"))?;
    println!(
        "      room_id={} estado={} título={:?}",
        lookup.room_id, lookup.status, lookup.title
    );
    if !lookup.is_live() {
        return Err(format!(
            "@{user} no está emitiendo (status={}). Firmar contra una sala apagada da un \
             protobuf sin push_server, que parece un rechazo: prueba con otro canal.",
            lookup.status
        ));
    }

    // --- 3. La firma -----------------------------------------------------------------
    // Antes hay que estar *en* la página del directo: es desde donde la hace el
    // reproductor real, y desde la portada /live la petición ni sale.
    let room_page = ttl_sign_core::room::live_page_url(&user);
    println!("\n[3/4] navegando a {room_page} …");
    signer
        .navigate(&room_page)
        .await
        .map_err(|e| format!("no se pudo cargar la página del directo: {e}"))?;

    println!("      firmando /webcast/im/fetch/ …");
    let signed_at = Instant::now();
    let signed = match signer.fetch(&lookup.room_id).await {
        SignOutcome::Ok(signed) => signed,
        SignOutcome::Rejected(reason) if !autenticado => {
            return Err(format!(
                "TikTok rechazó la firma: {reason}.\n\n\
                 La sesión es anónima, y eso hoy no basta: `/webcast/room/enter/` responde \
                 \"User doesn\'t login\" y `/webcast/im/fetch/` responde 200 con cuerpo vacío, \
                 que es el mismo rechazo dicho en voz baja. Por la misma ruta de firma, \
                 `room/info` y `room/check_alive` sí responden, así que ni la firma ni la \
                 repetición son el problema. Compruébalo con:\n\
                 \n    cargo run -p ttl-sign-webview --example endpoint-probe -- <usuario>\n\
                 \nPara probar autenticado, exporta la cookie `sessionid` de tu cuenta:\n\
                 \n    TTL_SESSION_ID=<sessionid> cargo run -p ttl-sign-webview --example live-check\n"
            ))
        }
        SignOutcome::Rejected(reason) => {
            return Err(format!(
                "TikTok rechazó la firma pese a la sesión autenticada: {reason}.\n\n\
                 Si esto funcionaba hace un rato y ahora no, lo más probable es el límite \
                 de tasa: firmar es interactuar con un antibot, y muchas firmas seguidas \
                 desde la misma IP y la misma sesión lo despiertan \
                 (docs/06-risks-and-ops.md §5). Espera un rato antes de insistir; \
                 reintentar en bucle lo empeora.\n\n\
                 Si nunca ha funcionado, mira la coherencia UA↔params (§1) y comprueba \
                 con: cargo run -p ttl-sign-webview --example endpoint-probe -- <usuario>"
            ))
        }
        SignOutcome::Transport(e) => return Err(format!("fallo de transporte: {e}")),
    };
    println!(
        "      {} bytes en {:?}, {} cookies",
        signed.protobuf.len(),
        signed_at.elapsed(),
        signed.cookies.len()
    );
    println!("      cookies: {}", signed.cookies); // redactadas

    let result = FetchResult::decode(&signed.protobuf)
        .map_err(|e| format!("el protobuf no se pudo decodificar: {e}. Confirma los números de campo en ttl-sign-core/src/proto.rs contra el fixture de F0"))?;
    println!("      push_server={}", result.push_server);
    println!("      route_params={} entradas", result.route_params.len());

    // --- 4. El WebSocket -------------------------------------------------------------
    // La URI se construye aquí y se firma en la página: el WS lleva firma propia, al
    // contrario de lo que decía docs/00 §1.
    println!("\n[4/4] firmando y conectando el WebSocket…");
    let config = ConnectConfig::default();
    let mut params = WsParams::new(&lookup.room_id);
    params.compress = config.compress.clone();
    params.cursor = result.cursor.clone();
    params.internal_ext = result.internal_ext.clone();
    let preset = signer.preset();
    let uri = params.build_uri(&result.push_server, &result.route_params, &preset);

    let uri = signer
        .sign_ws_uri(&uri)
        .await
        .map_err(|e| format!("no se pudo firmar la URI del WebSocket: {e}"))?;
    println!(
        "      firmada: {}",
        uri.split('?').next().unwrap_or_default()
    );

    let mut connection = LiveConnection::open_uri(
        &uri,
        &signed.cookies,
        &signed.user_agent,
        &result.internal_ext,
        &config,
    )
    .await
    .map_err(|e| format!("no se pudo abrir el WebSocket: {e}"))?;

    println!(
        "      conectado {:?} después de firmar",
        signed_at.elapsed()
    );

    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);
    let mut frames = 0usize;

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            msg = connection.next_message() => match msg {
                Some(Ok(m)) => {
                    frames += 1;
                    println!("      frame msg #{frames}: log_id={} {} bytes", m.log_id, m.payload.len());
                    if frames >= 3 {
                        break;
                    }
                }
                Some(Err(e)) => return Err(format!("el WebSocket falló: {e}")),
                None => break,
            }
        }
    }
    connection.close().await;

    if frames == 0 {
        return Err("conectó pero no llegó ningún frame `msg` en 30 s".into());
    }
    println!("\nOK: {frames} frames de @{user} con una firma propia. F2 y F4 pasan.");
    Ok(())
}

/// La página tarda en pintar los canales: se sondea el DOM en vez de leerlo una vez.
async fn poll_channels(signer: &Signer) -> Result<Vec<ttl_sign_core::LiveChannel>, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match signer.live_channels().await {
            Ok(channels) if !channels.is_empty() => return Ok(channels),
            Ok(_) => {}
            Err(e) => return Err(format!("no se pudo leer el DOM: {e}")),
        }
        if Instant::now() >= deadline {
            return Err("30 s y /live seguía sin listar canales".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
