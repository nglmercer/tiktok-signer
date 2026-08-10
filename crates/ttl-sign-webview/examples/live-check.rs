//! End-to-end verification against a real channel.
//!
//! Runs the complete flow and reports the step at which it fails:
//!
//! ```text
//! 1. discover who is live (rendered /live DOM, unsigned)
//! 2. unique_id → room_id + status (unsigned JSON lookup)
//! 3. /webcast/im/fetch/ ← the only signed endpoint
//! 4. open the WebSocket and receive frames
//! ```
//!
//! This is the F2 acceptance criterion (and F4 if frames arrive).
//!
//! ```sh
//! # discover a channel automatically
//! cargo run -p ttl-sign-webview --example live-check
//!
//! # or force a specific channel when you know it is live
//! cargo run -p ttl-sign-webview --example live-check -- user
//! ```
//!
//! Requires a display (X11/Wayland). Without one: `xvfb-run -a cargo run …`.

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
    // The flow currently requires authentication; see step 3.
    let (config, session_source) = configure_session();
    let authenticated = config.is_authenticated();
    println!("session: {session_source}");

    run(config, move |signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        let code = match rt.block_on(check(signer.clone(), requested_user, authenticated)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("\nFAILED: {e}");
                1
            }
        };
        signer.shutdown();
        std::process::exit(code);
    })
}

/// `TTL_SESSION_ID` takes precedence; otherwise use the session saved by `login`.
fn configure_session() -> (EngineConfig, String) {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return (
                EngineConfig::default().with_session_id(id),
                "authenticated (TTL_SESSION_ID)".into(),
            );
        }
    }
    if let Some(path) = session::configured_path() {
        if let Ok(Some(jar)) = session::load(&path) {
            if session::is_logged_in(&jar) {
                let source = format!("authenticated ({})", path.display());
                return (
                    EngineConfig {
                        session: jar,
                        ..EngineConfig::default()
                    },
                    source,
                );
            }
        }
    }
    (
        EngineConfig::default(),
        "anonymous — log in with: cargo run -p ttl-sign-webview --example login".into(),
    )
}

async fn check(
    signer: Signer,
    requested_user: Option<String>,
    authenticated: bool,
) -> Result<(), String> {
    // --- 1. Who is live? -------------------------------------------------------------
    let user = match requested_user {
        Some(user) => {
            println!("[1/4] manually selected channel: @{user}");
            user
        }
        None => {
            println!("[1/4] waiting for /live to render channels…");
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
                .ok_or("/live listed no channels (did it finish loading?)")?;
            println!("      selected: @{}", first.unique_id);
            first.unique_id.clone()
        }
    };

    // --- 2. unique_id → room_id ------------------------------------------------------
    println!("\n[2/4] resolving @{user} → room_id…");
    let lookup = signer
        .room_lookup(&user)
        .await
        .map_err(|e| format!("lookup failed: {e}"))?;
    println!(
        "      room_id={} status={} title={:?}",
        lookup.room_id, lookup.status, lookup.title
    );
    if !lookup.is_live() {
        return Err(format!(
            "@{user} is not live (status={}). Signing an offline room returns a protobuf \
             without push_server and looks like a rejection; try another channel.",
            lookup.status
        ));
    }

    // --- 3. Signing ------------------------------------------------------------------
    // We must first be *on* the live page: this is where the real player sends the request;
    // from the /live landing page it does not leave.
    let room_page = ttl_sign_core::room::live_page_url(&user);
    println!("\n[3/4] navigating to {room_page} …");
    signer
        .navigate(&room_page)
        .await
        .map_err(|e| format!("could not load live page: {e}"))?;

    println!("      signing /webcast/im/fetch/ …");
    let signed_at = Instant::now();
    let signed = match signer.fetch(&lookup.room_id).await {
        SignOutcome::Ok(signed) => signed,
        SignOutcome::Rejected(reason) if !authenticated => {
            return Err(format!(
                "TikTok rejected the signature: {reason}.\n\n\
                 The session is anonymous, which is currently insufficient: `/webcast/room/enter/` returns \
                 \"User doesn\'t login\" and `/webcast/im/fetch/` returns 200 with an empty body, \
                 which is the same silent rejection. The same signing path returns \
                 `room/info` and `room/check_alive`, so neither signing nor replay is the problem. Test with:\n\
                 \n    cargo run -p ttl-sign-webview --example endpoint-probe -- <user>\n\
                 \nTo test authenticated, export your account's `sessionid` cookie:\n\
                 \n    TTL_SESSION_ID=<sessionid> cargo run -p ttl-sign-webview --example live-check\n"
            ))
        }
        SignOutcome::Rejected(reason) => {
            return Err(format!(
                "TikTok rejected the signature despite authentication: {reason}.\n\n\
                 If this worked recently and no longer does, rate limiting is likely: signing \
                 interacts with anti-bot controls, and repeated signatures from one IP/session \
                 can trigger them. Wait before trying again; looping makes it worse.\n\n\
                 If it never worked, inspect UA↔params consistency and run: \
                 cargo run -p ttl-sign-webview --example endpoint-probe -- <user>"
            ))
        }
        SignOutcome::Transport(e) => return Err(format!("transport failure: {e}")),
    };
    println!(
        "      {} bytes in {:?}, {} cookies",
        signed.protobuf.len(),
        signed_at.elapsed(),
        signed.cookies.len()
    );
    println!("      cookies: {}", signed.cookies); // redacted

    let result = FetchResult::decode(&signed.protobuf)
        .map_err(|e| format!("could not decode protobuf: {e}. Confirm field numbers in ttl-sign-core/src/proto.rs against the F0 fixture"))?;
    println!("      push_server={}", result.push_server);
    println!("      route_params={} entries", result.route_params.len());

    // --- 4. WebSocket ----------------------------------------------------------------
    // The URI is built here and signed in the page: the WebSocket has its own signature.
    println!("\n[4/4] signing and connecting WebSocket…");
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
        .map_err(|e| format!("could not sign WebSocket URI: {e}"))?;
    println!(
        "      signed: {}",
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
    .map_err(|e| format!("could not open WebSocket: {e}"))?;

    println!("      connected {:?} after signing", signed_at.elapsed());

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
                Some(Err(e)) => return Err(format!("WebSocket failed: {e}")),
                None => break,
            }
        }
    }
    connection.close().await;

    if frames == 0 {
        return Err("connected but no `msg` frame arrived in 30 s".into());
    }
    println!("\nOK: {frames} frames from @{user} with an independent signature. F2 and F4 passed.");
    Ok(())
}

/// The page takes time to render channels, so poll the DOM instead of reading it once.
async fn poll_channels(signer: &Signer) -> Result<Vec<ttl_sign_core::LiveChannel>, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match signer.live_channels().await {
            Ok(channels) if !channels.is_empty() => return Ok(channels),
            Ok(_) => {}
            Err(e) => return Err(format!("could not read DOM: {e}")),
        }
        if Instant::now() >= deadline {
            return Err("/live still listed no channels after 30 s".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
