//! F1 — Validar el modelo de conexión **sin webview** (`docs/02-roadmap.md`).
//!
//! Reproduce a mano lo capturado en F0: repite la petición del fixture con `reqwest`,
//! extrae `push_server` / `route_params` / `cursor` / `internal_ext`, construye la URI
//! del WebSocket y conecta. Si llegan frames, el modelo de conexión es correcto y se
//! puede pasar a F2; si no, el problema está en el modelo y no en la firma.
//!
//! ```sh
//! # Repite la petición del fixture (los params firmados caducan en ~30 s:
//! # hay que capturar y ejecutar seguido)
//! cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl
//!
//! # Sin repetir la petición: usa el cuerpo protobuf ya capturado
//! cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl \
//!     --pb fixtures/f0/im_fetch.pb
//! ```

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ttl_live_ws::{ConnectConfig, LiveConnection};
use ttl_sign_core::{CookieJar, FetchResult, Preset, Query};

/// Lo que se puede sacar de un `Copy as cURL`.
#[derive(Debug, Default)]
struct CurlFixture {
    url: String,
    headers: Vec<(String, String)>,
    cookies: CookieJar,
}

impl CurlFixture {
    fn user_agent(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str())
    }

    fn room_id(&self) -> Option<String> {
        let (_, query) = self.url.split_once('?')?;
        Query::parse(query).get("room_id").map(str::to_owned)
    }
}

/// Parser de `Copy as cURL` suficiente para el fixture: URL, `-H` y `-b`.
///
/// Chrome emite las cabeceras con comillas simples y continuaciones de línea con `\`.
fn parse_curl(raw: &str) -> Result<CurlFixture> {
    let tokens = tokenize(raw);
    let mut fixture = CurlFixture::default();
    let mut i = 0;

    while i < tokens.len() {
        let token = &tokens[i];
        match token.as_str() {
            "curl" | "--compressed" | "--location" | "-L" | "-s" | "--silent" => i += 1,
            "-H" | "--header" => {
                let value = tokens.get(i + 1).context("-H sin valor")?;
                if let Some((k, v)) = value.split_once(':') {
                    let (k, v) = (k.trim(), v.trim());
                    if k.eq_ignore_ascii_case("cookie") {
                        fixture.cookies.merge(&CookieJar::parse(v));
                    } else {
                        fixture.headers.push((k.to_string(), v.to_string()));
                    }
                }
                i += 2;
            }
            "-b" | "--cookie" => {
                let value = tokens.get(i + 1).context("-b sin valor")?;
                fixture.cookies.merge(&CookieJar::parse(value));
                i += 2;
            }
            other if other.starts_with("http") => {
                fixture.url = other.to_string();
                i += 1;
            }
            // Cualquier otra opción con valor (`--data-raw`, `-X`, …): saltar ambos.
            other if other.starts_with('-') => i += 2,
            _ => i += 1,
        }
    }

    if fixture.url.is_empty() {
        bail!("no se encontró la URL en el fixture cURL");
    }
    Ok(fixture)
}

/// Trocea respetando comillas simples y dobles, e ignora las continuaciones `\<nl>`.
fn tokenize(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '\'') | (None, '"') => quote = Some(c),
            (None, '\\') => {
                // Continuación de línea: se traga el salto.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (None, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "replay=info,ttl_live_ws=debug".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let curl_path = args
        .next()
        .context("uso: replay <fixtures/f0/im_fetch.curl> [--pb <im_fetch.pb>]")?;
    let mut pb_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pb" => pb_path = Some(args.next().context("--pb sin ruta")?),
            other => bail!("argumento desconocido: {other}"),
        }
    }

    let raw = std::fs::read_to_string(&curl_path)
        .with_context(|| format!("no se pudo leer {curl_path}"))?;
    let fixture = parse_curl(&raw)?;

    let room_id = fixture
        .room_id()
        .context("la URL del fixture no trae room_id")?;
    let user_agent = fixture
        .user_agent()
        .context("el fixture no trae User-Agent; sin él el WS se rechaza")?
        .to_string();

    println!("room_id ......... {room_id}");
    println!("user-agent ...... {user_agent}");
    println!("cookies ......... {}", fixture.cookies); // Display redacta los valores.

    // --- Paso 2: la petición firmada -------------------------------------------------
    let signed_at = Instant::now();
    let protobuf = match &pb_path {
        Some(path) => {
            println!("\n[1/3] usando el protobuf capturado: {path}");
            std::fs::read(path).with_context(|| format!("no se pudo leer {path}"))?
        }
        None => {
            println!("\n[1/3] repitiendo la petición del fixture…");
            let client = reqwest::Client::builder().user_agent(&user_agent).build()?;
            let mut request = client.get(&fixture.url);
            for (k, v) in &fixture.headers {
                request = request.header(k, v);
            }
            let response = request
                .header("Cookie", fixture.cookies.to_cookie_string())
                .send()
                .await
                .context("la petición falló")?;

            let status = response.status();
            let body = response.bytes().await?.to_vec();
            println!("      HTTP {status}, {} bytes", body.len());
            if !status.is_success() {
                bail!("TikTok respondió {status}: el fixture ya no vale, recaptura F0");
            }
            if body.is_empty() {
                bail!("cuerpo vacío con {status}: rechazo silencioso, no es un fallo de red");
            }
            body
        }
    };

    // --- Paso 3: extraer los parámetros del WebSocket --------------------------------
    println!("\n[2/3] decodificando ProtoMessageFetchResult…");
    let result = FetchResult::decode(&protobuf).context("protobuf ilegible")?;
    println!("      push_server ..... {}", short(&result.push_server));
    println!(
        "      route_params .... {} entradas",
        result.route_params.len()
    );
    println!("      cursor .......... {}", short(&result.cursor));
    println!("      internal_ext .... {}", short(&result.internal_ext));

    if let Some(reason) = result.rejection_reason() {
        bail!(
            "la respuesta es un rechazo ({reason}). No es transitorio: no reintentar. \
             Revisa docs/00-research.md §1 antes de seguir."
        );
    }

    // --- Paso 4: abrir el WebSocket --------------------------------------------------
    println!("\n[3/3] conectando el WebSocket…");
    let mut connection = LiveConnection::open_with(
        &result,
        &fixture.cookies,
        &user_agent,
        &Preset::default(),
        &room_id,
        &ConnectConfig::default(),
    )
    .await
    .context("no se pudo abrir el WebSocket")?;

    println!(
        "      conectado en {:?} desde la firma",
        signed_at.elapsed()
    );
    if !connection.handshake_options().is_empty() {
        println!(
            "      handshake-options: {:?}",
            connection.handshake_options()
        );
    }
    if signed_at.elapsed() > Duration::from_secs(30) {
        println!("      aviso: más de 30 s desde la firma; si cierra ya, es caducidad");
    }

    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);
    let mut received = 0usize;

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            msg = connection.next_message() => match msg {
                Some(Ok(m)) => {
                    received += 1;
                    println!("      frame msg #{received}: log_id={}, {} bytes", m.log_id, m.payload.len());
                    if received >= 3 {
                        break;
                    }
                }
                Some(Err(e)) => bail!("el WebSocket falló: {e}"),
                None => break,
            }
        }
    }

    connection.close().await;

    if received == 0 {
        bail!("no llegó ningún frame `msg` en 30 s: F1 NO pasa");
    }
    println!("\nF1 PASA: {received} frames por el WebSocket con parámetros capturados a mano.");
    Ok(())
}

/// Recorta valores largos para que el log no sea ilegible ni filtre la cadena entera.
fn short(s: &str) -> String {
    if s.chars().count() <= 60 {
        return s.to_string();
    }
    format!("{}…", s.chars().take(60).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"curl 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000000&X-Gnarly=Kabc' \
  -H 'accept: */*' \
  -H 'user-agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0' \
  -b 'msToken=abc; tt-target-idc=useast1a' \
  --compressed"#;

    #[test]
    fn parses_a_chrome_curl() {
        let f = parse_curl(SAMPLE).unwrap();
        assert!(f.url.contains("X-Gnarly=Kabc"));
        assert_eq!(f.room_id().as_deref(), Some("7300000000000000000"));
        assert!(f.user_agent().unwrap().contains("Chrome/131"));
        assert_eq!(f.cookies.get("tt-target-idc"), Some("useast1a"));
    }

    #[test]
    fn cookie_header_lands_in_the_jar_not_in_the_headers() {
        let f = parse_curl("curl 'https://x/?room_id=1' -H 'Cookie: msToken=z'").unwrap();
        assert_eq!(f.cookies.get("msToken"), Some("z"));
        assert!(f
            .headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("cookie")));
    }
}
