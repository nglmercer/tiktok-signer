//! Paso 1 del flujo, sin webview y sin display: `unique_id` → `room_id` + estado.
//!
//! Sirve para tener un `room_id` **de una sala realmente en directo** antes de intentar
//! nada más. Firmar contra una sala apagada devuelve un protobuf sin `push_server`, que
//! es indistinguible de un rechazo: se depura el sitio equivocado durante un buen rato.
//!
//! ```sh
//! cargo run -p ttl-live-ws --example rooms -- usuario1 usuario2
//! ```
//!
//! Para descubrir *quién* está en directo hace falta el DOM renderizado de
//! `https://www.tiktok.com/live`, y eso necesita el webview:
//! `cargo run -p ttl-sign-webview --example live-check`.

use anyhow::{Context, Result};
use ttl_sign_core::room::{room_lookup_url, RoomLookup};
use ttl_sign_core::Preset;

#[tokio::main]
async fn main() -> Result<()> {
    let users: Vec<String> = std::env::args().skip(1).collect();
    if users.is_empty() {
        anyhow::bail!("uso: rooms <usuario> [usuario…]  (el @ inicial es opcional)");
    }

    let preset = Preset::default();
    let client = reqwest::Client::builder()
        .user_agent(preset.user_agent())
        .build()?;

    println!("{:<24} {:<22} {:<8} {}", "USUARIO", "ROOM_ID", "ESTADO", "TÍTULO");

    let mut live = 0usize;
    for user in &users {
        let url = room_lookup_url(user);
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("el lookup de @{user} falló"))?;
        let status = response.status();
        let body = response.text().await?;

        let Some(lookup) = RoomLookup::from_json(&body) else {
            println!(
                "{:<24} {:<22} {:<8} respuesta inesperada (HTTP {status})",
                user, "-", "?"
            );
            continue;
        };

        let state = if lookup.is_live() { "DIRECTO" } else { "off" };
        if lookup.is_live() {
            live += 1;
        }
        println!(
            "{:<24} {:<22} {:<8} {}",
            format!("@{}", lookup.unique_id),
            if lookup.room_id.is_empty() {
                "-"
            } else {
                &lookup.room_id
            },
            state,
            lookup.title
        );
    }

    println!("\n{live} de {} en directo.", users.len());
    if live == 0 {
        println!(
            "Sin ninguna sala en directo no se puede validar nada: el protobuf vendría \
             sin push_server y parecería un rechazo."
        );
    }
    Ok(())
}
