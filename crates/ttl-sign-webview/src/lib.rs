//! Motor de firma basado en webview.
//!
//! Implementa el **Plan A** de `docs/01-architecture.md` §D2: no se reimplementa el
//! algoritmo de firma, se deja que la propia página de TikTok firme y ejecute la
//! petición, y se recogen los bytes por IPC.
//!
//! # Modelo de hilos
//!
//! `wry`/`tao` exigen que el event loop viva en el hilo principal, y `run()` no retorna.
//! Por eso el punto de entrada es [`run`], que se queda con el hilo principal y entrega
//! un [`Signer`] a un worker:
//!
//! ```no_run
//! use ttl_sign_webview::{run, EngineConfig};
//!
//! run(EngineConfig::default(), |signer| {
//!     let rt = tokio::runtime::Runtime::new().unwrap();
//!     rt.block_on(async move {
//!         let outcome = signer.fetch("7300000000000000000").await;
//!         println!("{}", outcome.is_ok());
//!     });
//! });
//! ```
//!
//! `#[tokio::main]` en `main()` **no** sirve: el event loop tiene que estar en el hilo
//! principal. Es el error de integración más probable, de ahí que la API no permita
//! construir el motor de otra forma.

pub mod ipc;
pub mod session;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};
use wry::WebViewBuilder;

use ttl_sign_core::room::{self, LiveChannel, RoomLookup};
use ttl_sign_core::{
    CookieJar, FetchParams, FetchResult, Preset, RejectReason, SignError, SignOutcome, SignedFetch,
};

use crate::ipc::{FromPage, PageEnv, ToPage, ToPageText};

/// Cada cuánto se mira si ya hay sesión durante el login.
const POLL_LOGIN: Duration = Duration::from_secs(2);

/// Documento barato del origen TikTok usado para primear las cookies antes de la página
/// real. La primera navegación necesariamente ocurre antes de que un document-start script
/// pueda run; la segunda ya sale con la sesión puesta en la cabecera HTTP.
const SESSION_BOOTSTRAP_URL: &str = "https://www.tiktok.com/robots.txt";

/// Script inyectado antes que los de la página.
pub const BRIDGE_JS: &str = include_str!("bridge.js");

/// Configuración del motor.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Preset de dispositivo: genera el UA del webview **y** los params de la query.
    pub preset: Preset,
    /// Página sobre la que se firma. Tiene que ser una URL de TikTok que cargue
    /// `webmssdk.js`; una live cualquiera vale.
    pub landing_url: String,
    /// Tiempo máximo de una firma. Por encima de esto el resultado caducaría igualmente
    /// (~30 s de vida útil), así que fallar rápido es mejor que esperar.
    pub sign_timeout: Duration,
    /// Ventana para que aparezca `window.byted_acrawler`.
    pub sdk_ready_timeout: Duration,
    /// Email de contacto que pide la spec de Euler (`contact_us`). Opcional.
    pub contact_us: String,
    /// Ventana visible. Solo para el flujo de login, donde hace falta que la persona vea
    /// la página y pueda teclear; para firmar va invisible.
    pub visible: bool,
    /// Evita que el reproductor abra su propio WebSocket. El cliente Rust abre el socket
    /// con los parámetros devueltos por `/im/fetch/`; dejar dos sockets para la misma sala
    /// y sesión hace que TikTok acepte ambos pero deje al segundo en silencio.
    pub block_page_websockets: bool,
    /// Cookies de una cuenta real. Vacío = sesión anónima.
    ///
    /// Verificado el 2026-08-10 contra directos reales: **sin ella no hay flujo**.
    /// `/webcast/room/enter/` responde literalmente `User doesn't login`, y
    /// `/webcast/im/fetch/` responde 200 con cuerpo vacío, que es el mismo rechazo dicho
    /// en voz baja. Los endpoints que no dependen de sesión (`room/info`,
    /// `room/check_alive`) sí responden por esta misma ruta de firma, así que el problema
    /// no está en cómo firmamos ni en cómo repetimos.
    ///
    /// Implica exponer la sesión de una cuenta al componente que firma
    /// (`docs/06-risks-and-ops.md` §Decisiones abiertas). Con `sessionid`, el WebSocket
    /// **exige** además que la cookie viaje en el handshake, o rechaza con
    /// "illegal secret key".
    pub session: CookieJar,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            // En Linux el webview reporta `navigator.platform = "Linux x86_64"`, y
            // `with_user_agent` no lo cambia: un preset de Windows deja `browser_platform`
            // y el UA contando historias distintas, que es la causa nº1 de rechazo
            // (`docs/06-risks-and-ops.md` §1). Observado en F2.
            preset: Preset {
                device: default_device_preset(),
                ..Preset::default()
            },
            landing_url: "https://www.tiktok.com/live".into(),
            sign_timeout: Duration::from_secs(15),
            sdk_ready_timeout: Duration::from_secs(30),
            contact_us: String::new(),
            visible: false,
            block_page_websockets: true,
            session: CookieJar::new(),
        }
    }
}

impl EngineConfig {
    /// Configura la sesión a partir de una cookie `sessionid` suelta.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        let session_id = session_id.into();
        if !session_id.is_empty() {
            self.session.set(session::SESSION_COOKIE, session_id);
        }
        self
    }

    /// Configuración para el flujo de login: ventana visible, sin sesión previa y
    /// aterrizando en la página de login.
    pub fn for_login() -> Self {
        Self {
            visible: true,
            landing_url: "https://www.tiktok.com/login".into(),
            session: CookieJar::new(),
            ..Self::default()
        }
    }

    /// ¿Hay sesión configurada?
    pub fn is_authenticated(&self) -> bool {
        session::is_logged_in(&self.session)
    }
}

/// El preset que casa con el motor de webview de esta plataforma.
fn default_device_preset() -> ttl_sign_core::DevicePreset {
    if cfg!(target_os = "linux") {
        ttl_sign_core::DevicePreset::chrome_linux()
    } else if cfg!(target_os = "macos") {
        ttl_sign_core::DevicePreset::chrome_macos()
    } else {
        ttl_sign_core::DevicePreset::chrome_windows()
    }
}

/// Trabajo inyectado en el event loop desde el runtime de tokio.
enum UserEvent {
    Sign {
        url: String,
        reply: oneshot::Sender<Result<SignedRequest, SignError>>,
    },
    /// Paso 1 sin firma. `url: None` pide el DOM ya renderizado.
    Text {
        url: Option<String>,
        reply: oneshot::Sender<Result<String, SignError>>,
    },
    /// Cookies actuales del webview. No espera al gate: en la página de login el SDK
    /// puede no llegar a cargar nunca, y aun así hay sesión que leer.
    Cookies {
        reply: oneshot::Sender<CookieJar>,
    },
    /// Navega la página a otra URL y vuelve a cerrar el gate de readiness.
    Navigate {
        url: String,
        reply: oneshot::Sender<Result<(), SignError>>,
    },
    /// Despierta el event loop desde el handler de IPC.
    ///
    /// Hace falta porque el handler corre fuera del closure del event loop: cuando marca
    /// la instancia como lista, nadie vacía la cola de peticiones hasta que llega *algún*
    /// evento. Sin esto, todo lo encolado antes de `ready` se queda ahí hasta que caduca.
    Wake,
    Shutdown,
}

/// Lo que devuelve la página: una URL ya firmada y las cookies con las que se firmó.
/// El cuerpo lo trae Rust después, repitiendo esta petición.
#[derive(Debug, Clone)]
pub struct SignedRequest {
    /// URL con X-Bogus, X-Gnarly, X-Dynosaur y msToken puestos por webmssdk.
    pub url: String,
    pub cookies: CookieJar,
    pub user_agent: String,
    /// Página desde la que se firmó. Va como `Referer` al repetir.
    pub page_url: String,
}

/// Una petición `webcast` que el puente vio pasar, con su respuesta.
///
/// La graba el wrapper de `fetch`/XHR que se instala **antes** que webmssdk, así que la
/// URL ya viene firmada por el SDK y el cuerpo es el que recibió la página.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Capture {
    pub url: String,
    #[serde(default)]
    pub status: u16,
    /// `Response.type`: `cors` o `basic` se pueden leer; `opaque` no.
    #[serde(default, rename = "kind")]
    pub response_type: String,
    /// `fetch` o `xhr`.
    #[serde(default)]
    pub via: String,
    #[serde(default)]
    pub bytes: usize,
    /// Cuerpo en base64. Vacío si no se pudo leer.
    #[serde(default)]
    pub body: String,
    /// Cuerpo en claro cuando es corto y no se guardó en `body`: los errores de TikTok
    /// son JSON de dos líneas y leerlos ahorra media investigación.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl Capture {
    /// Cuerpo decodificado. Vacío si el puente no pudo leerlo.
    pub fn decoded(&self) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(&self.body)
            .unwrap_or_default()
    }

    pub fn endpoint(&self) -> &str {
        self.url.split('?').next().unwrap_or(&self.url)
    }
}

/// Handle async del motor. Clonable y `Send`: es lo que ve el servidor HTTP.
#[derive(Clone)]
pub struct Signer {
    proxy: EventLoopProxy<UserEvent>,
    config: EngineConfig,
    http: reqwest::Client,
    /// El preset **vivo**: el dispositivo lo fija la configuración (de él sale el UA del
    /// webview, que no se puede cambiar en caliente), pero idioma, zona horaria, pantalla
    /// y región los corrige la propia página en cuanto está lista.
    preset: Arc<Mutex<Preset>>,
}

impl Signer {
    /// Firma y ejecuta `/webcast/im/fetch/` para una sala.
    ///
    /// No devuelve `Result`: los tres finales posibles están en [`SignOutcome`], y ahí
    /// un rechazo (detección) nunca se confunde con un error de transporte.
    pub async fn fetch(&self, room_id: impl Into<String>) -> SignOutcome {
        let mut params = FetchParams::new(room_id);
        params.contact_us = self.config.contact_us.clone();
        self.fetch_with(params).await
    }

    /// Igual que [`Signer::fetch`], con control total sobre los parámetros (cursor,
    /// `internal_ext`).
    pub async fn fetch_with(&self, params: FetchParams) -> SignOutcome {
        let preset = self.preset();
        match self.sign_url(&params.url(&preset)).await {
            Ok(signed) => self.replay(signed).await,
            Err(e) => e.into(),
        }
    }

    /// Firma una URL de TikTok LIVE y la devuelve firmada, **sin ejecutarla**.
    ///
    /// Es el `GET /webcast/sign_url` que la spec de Euler marca como opcional, y la
    /// herramienta con la que se compara un endpoint contra otro cuando uno deja de
    /// responder.
    pub async fn sign_url(&self, url: &str) -> Result<SignedRequest, SignError> {
        let (tx, rx) = oneshot::channel();
        self.proxy
            .send_event(UserEvent::Sign {
                url: url.to_string(),
                reply: tx,
            })
            .map_err(|_| SignError::EngineGone("el event loop se ha cerrado".into()))?;

        match tokio::time::timeout(self.config.sign_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(SignError::EngineGone(
                "el motor descartó la petición".into(),
            )),
            Err(_) => Err(SignError::Timeout(
                self.config.sign_timeout.as_millis() as u64
            )),
        }
    }

    /// Repite una petición firmada y devuelve status y bytes crudos, sin interpretarlos.
    pub async fn replay_raw(&self, signed: &SignedRequest) -> Result<(u16, Vec<u8>), SignError> {
        let mut signed = signed.clone();

        // El `msToken` de la query y el de la cookie tienen que ser el mismo. Al firmar,
        // la petición sale desde el navegador y TikTok responde con un `x-ms-token`
        // nuevo, así que la cookie del webview ya ha rotado cuando la leemos: mandaríamos
        // la cookie nueva con una query firmada con la vieja.
        if let Some(query_token) = query_param(&signed.url, "msToken") {
            signed.cookies.set("msToken", query_token);
        }

        // Cabeceras de navegador, no de cliente HTTP: se replican las del reproductor
        // real, incluido `Accept: */*` (no `application/protobuf`), el `Referer` de la
        // página del directo y unos client hints coherentes con el UA.
        let referer = if signed.page_url.is_empty() {
            "https://www.tiktok.com/".to_string()
        } else {
            signed.page_url.clone()
        };

        let preset = self.preset();
        let response = self
            .http
            .get(&signed.url)
            .header("User-Agent", &signed.user_agent)
            .header("Cookie", signed.cookies.to_cookie_string())
            .header("Accept", "*/*")
            .header("Accept-Language", &preset.location.browser_language)
            .header("Referer", &referer)
            .header("Origin", "https://www.tiktok.com")
            .header("Sec-Fetch-Site", "same-site")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Dest", "empty")
            .header(
                "sec-ch-ua",
                "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"",
            )
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", platform_hint(&preset))
            .send()
            .await
            .map_err(|e| SignError::Transport(e.to_string()))?;

        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| SignError::Transport(e.to_string()))?
            .to_vec();
        Ok((status, bytes))
    }

    /// Repite la petición firmada e interpreta el resultado.
    ///
    /// Es el Plan B de `docs/01-architecture.md` §D2: la respuesta de
    /// `/webcast/im/fetch/` no se puede leer desde la página, y fuera del navegador esa
    /// restricción no existe.
    async fn replay(&self, signed: SignedRequest) -> SignOutcome {
        debug!(
            // Sin la query: lleva msToken y X-Gnarly en claro.
            endpoint = %signed.url.split('?').next().unwrap_or_default(),
            cookies = ?signed.cookies.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            "repitiendo la petición firmada desde Rust"
        );
        match self.replay_raw(&signed).await {
            Ok((status, body)) => build_outcome(
                status,
                &signed.url,
                body,
                signed.cookies,
                &signed.user_agent,
            ),
            Err(e) => e.into(),
        }
    }

    /// Cookies actuales del webview, incluidas las `HttpOnly`.
    ///
    /// Contienen la sesión: no imprimirlas en claro.
    pub async fn cookies(&self) -> Result<CookieJar, SignError> {
        let (tx, rx) = oneshot::channel();
        self.proxy
            .send_event(UserEvent::Cookies { reply: tx })
            .map_err(|_| SignError::EngineGone("el event loop se ha cerrado".into()))?;
        rx.await
            .map_err(|_| SignError::EngineGone("el motor descartó la petición".into()))
    }

    /// Espera a que la persona inicie sesión en la ventana, hasta `timeout`.
    ///
    /// Sondea las cookies del webview buscando `sessionid`. Devuelve las cookies de
    /// sesión ya filtradas, listas para guardar con [`session::save`].
    ///
    /// `on_tick` recibe el tiempo restante en cada sondeo; sirve para ir informando sin
    /// que este crate imprima nada por su cuenta.
    pub async fn wait_for_login(
        &self,
        timeout: Duration,
        mut on_tick: impl FnMut(Duration),
    ) -> Result<CookieJar, SignError> {
        let deadline = Instant::now() + timeout;
        loop {
            let jar = self.cookies().await?;
            if session::is_logged_in(&jar) {
                return Ok(session::filter(&jar));
            }
            let restante = deadline.saturating_duration_since(Instant::now());
            if restante.is_zero() {
                return Err(SignError::LoginTimeout(timeout.as_secs()));
            }
            on_tick(restante);
            tokio::time::sleep(POLL_LOGIN.min(restante)).await;
        }
    }

    /// Lleva el webview a otra página y espera a que el puente vuelva a estar listo.
    ///
    /// Hace falta para firmar: la petición `/webcast/im/fetch/` solo sale adelante desde
    /// la página del propio directo (`https://www.tiktok.com/@usuario/live`), que es
    /// donde la hace el reproductor real. Desde la portada `/live` el `fetch` parcheado
    /// resuelve a `undefined` y el XHR sale con status 0: la petición ni se emite.
    /// Verificado en F2.
    pub async fn navigate(&self, url: &str) -> Result<(), SignError> {
        let (tx, rx) = oneshot::channel();
        if self
            .proxy
            .send_event(UserEvent::Navigate {
                url: url.to_string(),
                reply: tx,
            })
            .is_err()
        {
            return Err(SignError::EngineGone("el event loop se ha cerrado".into()));
        }
        match tokio::time::timeout(self.config.sdk_ready_timeout, rx).await {
            Ok(Ok(result)) => result?,
            Ok(Err(_)) => {
                return Err(SignError::EngineGone(
                    "el motor descartó la navegación".into(),
                ))
            }
            Err(_) => {
                return Err(SignError::Timeout(
                    self.config.sdk_ready_timeout.as_millis() as u64,
                ))
            }
        }
        // Cualquier petición espera al gate: pedir el DOM equivale a esperar a `ready`.
        self.dom().await.map(|_| ())
    }

    /// Resuelve `unique_id` → `room_id` y estado del directo. **Sin firma**
    /// (`docs/00-research.md` §1).
    ///
    /// Se hace desde la página para reutilizar sesión y UA reales, aunque el endpoint
    /// no lo exija.
    pub async fn room_lookup(&self, unique_id: &str) -> Result<RoomLookup, SignError> {
        let body = self.fetch_text(&room::room_lookup_url(unique_id)).await?;
        RoomLookup::from_json(&body).ok_or_else(|| {
            SignError::Decode(format!("respuesta inesperada del lookup de @{unique_id}"))
        })
    }

    /// Canales en directo, leídos del DOM **ya renderizado** de la página actual.
    ///
    /// El motor tiene que estar cargado en una página que los liste (por defecto
    /// `https://www.tiktok.com/live`). Un `GET` pelado a esa URL no sirve: los datos los
    /// pinta el cliente, así que el HTML crudo llega vacío.
    ///
    /// Devuelve la lista tal cual: si está vacía, es que la página aún no había pintado
    /// nada, no que no haya nadie emitiendo.
    pub async fn live_channels(&self) -> Result<Vec<LiveChannel>, SignError> {
        let dom = self.dom().await?;
        Ok(room::extract_live_channels(&dom))
    }

    /// DOM renderizado de la página actual.
    pub async fn dom(&self) -> Result<String, SignError> {
        self.text_request(None).await
    }

    /// Firma la URI de un WebSocket.
    ///
    /// Corrige lo que decía `docs/00-research.md` §1 ("el WS no lleva firma propia"): la
    /// URI que abre el reproductor real lleva `X-Gnarly`. Sin ella el handshake se acepta
    /// igual —101, `handshake-msg: OK`— y luego no llega ni un frame, que es el fallo más
    /// difícil de diagnosticar de todo el flujo.
    ///
    /// Se firma pasando la **misma query** por el firmador de peticiones HTTP, con el
    /// esquema cambiado a `https`, y devolviendo el `wss` con los parámetros que el SDK
    /// haya añadido. Tres caminos, en este orden:
    ///
    /// 1. La URI que ya construyó el reproductor en esta misma página. Es la referencia
    ///    exacta y evita gastar otra firma para un GET artificial al host WS.
    /// 2. El firmador de `fetch` (el mismo que usa `/webcast/im/fetch/`), que es quien
    ///    pone `X-Gnarly`.
    /// 3. `byted_acrawler.frontierSign`, que existe pero hoy solo devuelve `X-Bogus`.
    ///    Se conserva como último respaldo y porque es la API declarada para esto.
    pub async fn sign_ws_uri(&self, uri: &str) -> Result<String, SignError> {
        let (scheme, resto) = match uri.split_once("://") {
            Some((scheme, resto)) => (scheme, resto),
            None => return Err(SignError::Decode(format!("URI sin esquema: {uri}"))),
        };

        // The page's constructor wrapper records the URL before optionally blocking the
        // connection, so this remains available even when Rust owns the only real socket.
        if let Some(page_uri) = self.page_ws_candidate(uri).await {
            debug!(
                endpoint = %uri.split('?').next().unwrap_or(uri),
                "reutilizando URI WS firmada por la página"
            );
            return Ok(page_uri);
        }

        let como_https = format!("https://{resto}");

        // 2. Firmador de peticiones: es el que añade X-Gnarly.
        if let Ok(signed) = self.sign_url(&como_https).await {
            if signed.url.contains("X-Gnarly=") {
                let sin_esquema = signed
                    .url
                    .split_once("://")
                    .map(|(_, resto)| resto)
                    .unwrap_or(&signed.url);
                return Ok(format!("{scheme}://{sin_esquema}"));
            }
        }

        // 3. Respaldo: la API declarada del SDK.
        let url_json = serde_json::to_string(uri).map_err(|e| SignError::Decode(e.to_string()))?;
        let script = format!(
            "JSON.stringify(window.byted_acrawler.frontierSign({{ url: {url_json} }}) || {{}})"
        );
        let raw = self.eval(&script).await?;
        let params: std::collections::BTreeMap<String, String> = serde_json::from_str(&raw)
            .map_err(|e| SignError::Decode(format!("frontierSign devolvió algo raro: {e}")))?;
        if params.is_empty() {
            return Err(SignError::Bridge(
                "nadie firmó la URI del WebSocket: ni el firmador de fetch ni frontierSign".into(),
            ));
        }

        let mut signed = uri.to_string();
        for (key, value) in params {
            signed.push('&');
            signed.push_str(&key);
            signed.push('=');
            signed.push_str(&urlencode(&value));
        }
        Ok(signed)
    }

    /// Lo que el puente grabó de las peticiones `webcast` que hizo la **propia página**.
    ///
    /// Es la verdad de referencia: el reproductor real firma con los parámetros correctos
    /// y con la sesión correcta, así que comparar contra esto separa "construimos mal la
    /// petición" de "TikTok nos rechaza".
    pub async fn captures(&self) -> Result<Vec<Capture>, SignError> {
        let raw = self
            .eval("JSON.stringify(window.__ttlCaptures||[])")
            .await?;
        serde_json::from_str(&raw)
            .map_err(|e| SignError::Decode(format!("capturas ilegibles: {e}")))
    }

    /// URIs de WebSocket que ha abierto la página, en orden.
    pub async fn page_ws_urls(&self) -> Result<Vec<String>, SignError> {
        let raw = self.eval("JSON.stringify(window.__ttlWsUrls||[])").await?;
        serde_json::from_str(&raw)
            .map_err(|e| SignError::Decode(format!("lista de WebSockets ilegible: {e}")))
    }

    async fn page_ws_candidate(&self, uri: &str) -> Option<String> {
        let expected_endpoint = uri.split_once('?').map(|(base, _)| base).unwrap_or(uri);
        let expected_room = query_param(uri, "room_id");
        for attempt in 0..10 {
            if let Ok(urls) = self.page_ws_urls().await {
                if let Some(page_uri) = urls.into_iter().rev().find(|page_uri| {
                    page_uri
                        .split_once('?')
                        .map(|(base, query)| {
                            let same_room = match (&expected_room, query_param(page_uri, "room_id"))
                            {
                                (Some(expected), Some(actual)) => {
                                    actual.as_str() == expected.as_str()
                                }
                                (None, _) => true,
                                _ => false,
                            };
                            base == expected_endpoint && same_room && query.contains("X-Gnarly=")
                        })
                        .unwrap_or(false)
                }) {
                    return Some(page_uri);
                }
            }
            if attempt != 9 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        None
    }

    /// Impide que la página abra sus propios WebSockets a partir de ahora.
    ///
    /// Dos conexiones a la misma sala con la misma sesión se estorban: el servidor acepta
    /// la segunda y no le manda nada. Si el WebSocket lo va a abrir Rust, el de la página
    /// sobra.
    pub async fn block_page_websockets(&self) -> Result<(), SignError> {
        self.eval("(window.__ttlBlockWs=true,'ok')")
            .await
            .map(|_| ())
    }

    /// Evalúa una expresión JS en la página y devuelve el resultado como texto.
    ///
    /// Herramienta de diagnóstico: sirve para ver qué está pidiendo la página realmente
    /// o si un símbolo del SDK sigue existiendo. No participa en el camino de la firma.
    pub async fn eval(&self, js: &str) -> Result<String, SignError> {
        self.text_request(Some(format!("js:{js}"))).await
    }

    /// GET de texto desde dentro de la página, con su sesión y su UA.
    pub async fn fetch_text(&self, url: &str) -> Result<String, SignError> {
        self.text_request(Some(url.to_string())).await
    }

    async fn text_request(&self, url: Option<String>) -> Result<String, SignError> {
        let (tx, rx) = oneshot::channel();
        if self
            .proxy
            .send_event(UserEvent::Text { url, reply: tx })
            .is_err()
        {
            return Err(SignError::EngineGone("el event loop se ha cerrado".into()));
        }
        match tokio::time::timeout(self.config.sign_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(SignError::EngineGone(
                "el motor descartó la petición".into(),
            )),
            Err(_) => Err(SignError::Timeout(
                self.config.sign_timeout.as_millis() as u64
            )),
        }
    }

    /// El preset con el que firma este motor. El WebSocket tiene que usar este mismo UA.
    ///
    /// Se devuelve una copia porque la página puede corregir idioma, región, zona horaria
    /// y pantalla cuando emite `ready`; mantener una referencia a un `Mutex` no sería una
    /// API segura para el worker.
    pub fn preset(&self) -> Preset {
        self.preset
            .lock()
            .map(|preset| preset.clone())
            .unwrap_or_else(|_| self.config.preset.clone())
    }

    /// Pide el cierre ordenado del event loop.
    pub fn shutdown(&self) {
        let _ = self.proxy.send_event(UserEvent::Shutdown);
    }
}

/// Una firma esperando a que el SDK esté listo.
type QueuedSign = (String, oneshot::Sender<Result<SignedRequest, SignError>>);

/// Una petición de texto esperando a que la página esté lista.
type QueuedText = (Option<String>, oneshot::Sender<Result<String, SignError>>);

/// Estado compartido entre el event loop y el handler de IPC. Ambos corren en el hilo
/// principal, así que `Rc<RefCell<_>>` es suficiente y evita locks.
#[derive(Default)]
struct Shared {
    next_id: u64,
    /// Peticiones en vuelo, por `request_id`.
    pending: HashMap<u64, oneshot::Sender<Result<SignedRequest, SignError>>>,
    /// Peticiones de texto en vuelo (paso 1, sin firma).
    pending_text: HashMap<u64, oneshot::Sender<Result<String, SignError>>>,
    /// Peticiones recibidas antes de que el SDK estuviese listo.
    queued: Vec<QueuedSign>,
    /// Igual, para las de texto: `ready` implica además que el documento ya cargó, que
    /// es lo que hace falta para que el DOM tenga algo que leer.
    queued_text: Vec<QueuedText>,
    ready: bool,
    /// Cuándo empezó a contar el gate de readiness. Se reinicia en cada navegación.
    gate_started: Option<Instant>,
    sdk_version: Option<String>,
    signs: u64,
    /// Cookies visibles al documento. En WebKitGTK pueden no aparecer en el cookie
    /// manager aunque sí viajen en las peticiones hechas desde la página.
    page_cookies: CookieJar,
    /// Hay una navegación inicial a `robots.txt` que solo existe para que el puente pueda
    /// poner las cookies en el origen TikTok. Mientras está pendiente, un `ready` del
    /// documento de arranque no autoriza firmas.
    session_bootstrap_pending: bool,
    session_bootstrap_ready: bool,
}

impl Shared {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn resolve(&mut self, request_id: u64, result: Result<SignedRequest, SignError>) {
        match self.pending.remove(&request_id) {
            Some(tx) => {
                let _ = tx.send(result);
            }
            None => debug!(request_id, "resultado sin petición en vuelo, se descarta"),
        }
    }

    fn resolve_text(&mut self, request_id: u64, result: Result<String, SignError>) {
        match self.pending_text.remove(&request_id) {
            Some(tx) => {
                let _ = tx.send(result);
            }
            None => debug!(request_id, "texto sin petición en vuelo, se descarta"),
        }
    }

    /// Falla todo lo que estuviera esperando. Se usa cuando el SDK no arranca.
    fn fail_all(&mut self, make_error: impl Fn() -> SignError) {
        for (_, tx) in self.pending.drain() {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.queued.drain(..) {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.pending_text.drain() {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.queued_text.drain(..) {
            let _ = tx.send(Err(make_error()));
        }
    }
}

/// Arranca el motor. **Se queda con el hilo principal y no retorna.**
///
/// `worker` corre en un hilo aparte y recibe el [`Signer`]; es donde va el runtime de
/// tokio (servidor HTTP, cliente propio, lo que sea).
pub fn run<F>(config: EngineConfig, worker: F) -> !
where
    F: FnOnce(Signer) + Send + 'static,
{
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(if config.visible {
            "Inicia sesión en TikTok — se cerrará solo al terminar"
        } else {
            "ttl-sign-webview"
        })
        .with_visible(config.visible)
        .build(&event_loop)
        .expect("no se pudo crear la ventana (¿hay display? si no, usa Xvfb)");

    let shared = Rc::new(RefCell::new(Shared::default()));
    let user_agent = config.preset.user_agent();
    let preset = Arc::new(Mutex::new(config.preset.clone()));
    shared.borrow_mut().session_bootstrap_pending = !config.session.is_empty();

    let ipc_shared = Rc::clone(&shared);
    let ipc_ua = user_agent.clone();
    let ipc_preset = Arc::clone(&preset);
    let ipc_proxy = proxy.clone();
    // El webview aún no existe cuando se construye el handler, y el handler necesita
    // leer el cookie manager: se comparte por una celda que se rellena justo después.
    let webview_slot: Rc<RefCell<Option<Rc<wry::WebView>>>> = Rc::new(RefCell::new(None));
    let ipc_slot = Rc::clone(&webview_slot);

    // With a session, the first document is only a cookie bootstrap. Its response can be
    // anonymous, but its document-start script runs on the correct TikTok origin; after it
    // reports `session`, the event loop navigates to the real landing URL.
    let session_script =
        session_initialization_script(&config.session, config.block_page_websockets);
    let initial_url = if config.session.is_empty() {
        config.landing_url.as_str()
    } else {
        SESSION_BOOTSTRAP_URL
    };
    let builder = WebViewBuilder::new()
        .with_url(initial_url)
        .with_user_agent(&user_agent)
        .with_initialization_script(session_script)
        .with_initialization_script(BRIDGE_JS)
        .with_ipc_handler(move |req| {
            let body = req.body().as_str();
            let msg: FromPage = match serde_json::from_str(body) {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "mensaje IPC ilegible");
                    return;
                }
            };
            let jar_from_webview = ipc_slot
                .borrow()
                .as_ref()
                .map(|wv| cookies_from_webview(wv))
                .unwrap_or_default();
            handle_page_message(&ipc_shared, msg, &ipc_ua, jar_from_webview, &ipc_preset);
            // El handler no es el event loop: hay que despertarlo para que drene la cola.
            let _ = ipc_proxy.send_event(UserEvent::Wake);
        });

    let webview = build_webview(builder, &window).expect("no se pudo crear el webview");

    let webview = Rc::new(webview);
    *webview_slot.borrow_mut() = Some(Rc::clone(&webview));

    if !config.session.is_empty() {
        info!(
            cookies = config.session.len(),
            "sesión preparada para el documento de arranque"
        );
    }

    std::thread::spawn({
        let signer = Signer {
            proxy,
            config: config.clone(),
            http: http_client(&user_agent),
            preset: Arc::clone(&preset),
        };
        move || worker(signer)
    });

    shared.borrow_mut().gate_started = Some(Instant::now());
    let sdk_ready_timeout = config.sdk_ready_timeout;
    let landing_url = config.landing_url.clone();

    event_loop.run(move |event, _target, control_flow| {
        // Wait despierta con cada evento; el deadline del SDK se comprueba en cada paso.
        *control_flow = ControlFlow::Wait;

        {
            let mut state = shared.borrow_mut();
            let waiting = !state.queued.is_empty() || !state.queued_text.is_empty();
            let expired = state
                .gate_started
                .is_some_and(|t| t.elapsed() > sdk_ready_timeout);
            if !state.ready && waiting && expired {
                error!("el SDK no apareció en {sdk_ready_timeout:?}");
                state.fail_all(|| SignError::SdkNotReady);
            }
        }

        match event {
            Event::UserEvent(UserEvent::Sign { url, reply }) => {
                let mut state = shared.borrow_mut();
                if !state.ready {
                    debug!("petición en cola: el SDK todavía no está listo");
                    state.queued.push((url, reply));
                    return;
                }
                let id = state.next_id();
                state.pending.insert(id, reply);
                state.signs += 1;
                drop(state);
                dispatch(&webview, &shared, id, url);
            }
            Event::UserEvent(UserEvent::Text { url, reply }) => {
                let mut state = shared.borrow_mut();
                if !state.ready {
                    debug!("petición de texto en cola: la página aún no está lista");
                    state.queued_text.push((url, reply));
                    return;
                }
                let id = state.next_id();
                state.pending_text.insert(id, reply);
                drop(state);
                dispatch_text(&webview, &shared, id, url);
            }
            Event::UserEvent(UserEvent::Cookies { reply }) => {
                let mut jar = cookies_from_webview(&webview);
                jar.merge(&shared.borrow().page_cookies);
                let _ = reply.send(jar);
            }
            Event::UserEvent(UserEvent::Navigate { url, reply }) => {
                {
                    let mut state = shared.borrow_mut();
                    state.ready = false;
                    state.gate_started = Some(Instant::now());
                }
                info!(%url, "navegando");
                let result = webview
                    .load_url(&url)
                    .map_err(|e| SignError::EngineGone(e.to_string()));
                let _ = reply.send(result);
            }
            // Solo sirve para llegar al drenaje de colas de más abajo.
            Event::UserEvent(UserEvent::Wake) => {}
            Event::UserEvent(UserEvent::Shutdown) => {
                shared
                    .borrow_mut()
                    .fail_all(|| SignError::EngineGone("cierre solicitado".into()));
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }

        // Do this on the event-loop thread, never from the IPC callback. The callback has
        // already observed the cookies, while this navigation is the first request that
        // can carry them in its HTTP Cookie header.
        let navigate_after_bootstrap = {
            let mut state = shared.borrow_mut();
            if state.session_bootstrap_pending && state.session_bootstrap_ready {
                state.session_bootstrap_pending = false;
                state.session_bootstrap_ready = false;
                state.ready = false;
                state.gate_started = Some(Instant::now());
                true
            } else {
                false
            }
        };
        if navigate_after_bootstrap {
            info!(url = %landing_url, "sesión instalada; navegando a la página real");
            if let Err(e) = webview.load_url(&landing_url) {
                error!(error = %e, "no se pudo cargar la página autenticada");
                shared
                    .borrow_mut()
                    .fail_all(|| SignError::EngineGone(e.to_string()));
            }
        }

        // El puente puede haber emitido `ready` desde el handler de IPC mientras había
        // peticiones en cola; es aquí donde se drenan.
        let to_dispatch: Vec<(u64, String)> = {
            let mut state = shared.borrow_mut();
            if !state.ready || state.queued.is_empty() {
                Vec::new()
            } else {
                let queued: Vec<_> = state.queued.drain(..).collect();
                let mut out = Vec::with_capacity(queued.len());
                for (url, reply) in queued {
                    let id = state.next_id();
                    state.pending.insert(id, reply);
                    state.signs += 1;
                    out.push((id, url));
                }
                out
            }
        };
        for (id, url) in to_dispatch {
            dispatch(&webview, &shared, id, url);
        }

        let text_to_dispatch: Vec<(u64, Option<String>)> = {
            let mut state = shared.borrow_mut();
            if !state.ready || state.queued_text.is_empty() {
                Vec::new()
            } else {
                let queued: Vec<_> = state.queued_text.drain(..).collect();
                let mut out = Vec::with_capacity(queued.len());
                for (url, reply) in queued {
                    let id = state.next_id();
                    state.pending_text.insert(id, reply);
                    out.push((id, url));
                }
                out
            }
        };
        for (id, url) in text_to_dispatch {
            dispatch_text(&webview, &shared, id, url);
        }
    })
}

/// Prepara los pares que el puente instalará desde el origen de TikTok.
///
/// En WebKitGTK `WebView::set_cookie` puede quedar en el cookie manager sin participar en
/// la primera navegación. La asignación desde el initialization script ocurre en el mismo
/// origen y antes de que TikTok ejecute su bootstrap, por lo que las cookies sí viajan en
/// el SSR y en los XHR posteriores.
fn session_initialization_script(jar: &CookieJar, block_page_websockets: bool) -> String {
    let pairs: Vec<(&str, &str)> = jar.iter().collect();
    let json = serde_json::to_string(&pairs).expect("las cookies siempre serializan");
    format!("window.__ttlSession = {json}; window.__ttlBlockWs = {block_page_websockets};")
}

/// En Linux la ventana es invisible, así que GTK nunca la *realiza* y no hay window
/// handle que darle a `build()`: falla con `WindowHandleError(Unavailable)`. La forma
/// correcta ahí es meter el webview directamente en el contenedor GTK de la ventana.
#[cfg(target_os = "linux")]
fn build_webview(
    builder: WebViewBuilder<'_>,
    window: &tao::window::Window,
) -> wry::Result<wry::WebView> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;

    let vbox = window
        .default_vbox()
        .expect("tao siempre crea un vbox en GTK");
    builder.build_gtk(vbox)
}

#[cfg(not(target_os = "linux"))]
fn build_webview(
    builder: WebViewBuilder<'_>,
    window: &tao::window::Window,
) -> wry::Result<wry::WebView> {
    builder.build(window)
}

/// Lanza una firma dentro de la página.
fn dispatch(webview: &wry::WebView, shared: &Rc<RefCell<Shared>>, request_id: u64, url: String) {
    let msg = ToPage { request_id, url };
    debug!(request_id, "firma solicitada a la página");
    if let Err(e) = webview.evaluate_script(&msg.to_script()) {
        error!(request_id, error = %e, "no se pudo inyectar la firma");
        shared
            .borrow_mut()
            .resolve(request_id, Err(SignError::EngineGone(e.to_string())));
    }
}

/// Lanza una petición de texto (o de DOM, si `url` es `None`) dentro de la página.
fn dispatch_text(
    webview: &wry::WebView,
    shared: &Rc<RefCell<Shared>>,
    request_id: u64,
    url: Option<String>,
) {
    let msg = ToPageText { request_id, url };
    if let Err(e) = webview.evaluate_script(&msg.to_script()) {
        error!(request_id, error = %e, "no se pudo inyectar la petición de texto");
        shared
            .borrow_mut()
            .resolve_text(request_id, Err(SignError::EngineGone(e.to_string())));
    }
}

/// Cookies del cookie manager de WebKit, que **sí** incluye las `HttpOnly`
/// (`docs/04-spec-webview-bridge.md` §Cookies). `document.cookie` no las ve.
fn cookies_from_webview(webview: &wry::WebView) -> CookieJar {
    match webview.cookies() {
        Ok(cookies) => cookies
            .iter()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect(),
        Err(e) => {
            warn!(error = %e, "no se pudieron leer las cookies del webview");
            CookieJar::new()
        }
    }
}

/// Traduce un mensaje del puente a un [`SignOutcome`] y resuelve al que espera.
fn handle_page_message(
    shared: &Rc<RefCell<Shared>>,
    msg: FromPage,
    user_agent: &str,
    jar_from_webview: CookieJar,
    preset: &Arc<Mutex<Preset>>,
) {
    match msg {
        FromPage::Ready { sdk_version, env } => {
            if let Some(env) = env {
                apply_page_env(preset, &env);
            }
            let mut state = shared.borrow_mut();
            // `robots.txt` has no SDK of its own, but keep this guard in case the bootstrap
            // document happens to expose one through a cached page.
            state.ready = !state.session_bootstrap_pending;
            state.sdk_version = sdk_version.clone();
            info!(sdk_version = ?sdk_version, "puente listo");
        }
        FromPage::Session {
            installed,
            host,
            cookie,
        } => {
            let mut state = shared.borrow_mut();
            let document_cookies = CookieJar::parse(&cookie);
            let has_session = session::is_logged_in(&document_cookies);
            state.page_cookies.merge(&document_cookies);
            if state.session_bootstrap_pending {
                state.session_bootstrap_ready =
                    installed != 0 && has_session && host.ends_with("tiktok.com");
            }
            debug!(installed, %host, "cookies de sesión ofrecidas al documento");
        }
        FromPage::Text {
            request_id,
            status,
            body,
        } => {
            let result = if status == 200 {
                Ok(body)
            } else {
                // Sin firma de por medio, un no-200 aquí es un fallo normal del paso 1,
                // no una detección: no toca `SignOutcome::Rejected`.
                Err(SignError::Transport(format!(
                    "el paso sin firma respondió HTTP {status}"
                )))
            };
            shared.borrow_mut().resolve_text(request_id, result);
        }
        FromPage::Error {
            request_id,
            message,
        } => {
            if request_id == 0 {
                // No corresponde a ninguna firma: es el gate de readiness.
                error!(%message, "el puente falló antes de estar listo");
                let mut state = shared.borrow_mut();
                state.ready = false;
                state.fail_all(|| SignError::SdkNotReady);
            } else {
                // Puede ser de una firma o de una petición de texto: solo uno de los dos
                // mapas la tiene.
                let mut state = shared.borrow_mut();
                if state.pending_text.contains_key(&request_id) {
                    state.resolve_text(request_id, Err(SignError::Bridge(message)));
                } else {
                    state.resolve(request_id, Err(SignError::Bridge(message)));
                }
            }
        }
        FromPage::Signed {
            request_id,
            url,
            cookie,
            page,
        } => {
            // El manager aporta las HttpOnly; la cookie del documento se aplica después
            // porque es la que refleja la rotación más reciente de `msToken` y de la sesión
            // inyectada en WebKitGTK.
            let mut cookies = jar_from_webview;
            cookies.merge(&CookieJar::parse(&cookie));
            let signed = SignedRequest {
                url,
                cookies,
                user_agent: user_agent.to_string(),
                page_url: page,
            };
            shared.borrow_mut().resolve(request_id, Ok(signed));
        }
    }
}

/// Alinea los parámetros que firma Rust con el entorno que ve el webview real.
fn apply_page_env(preset: &Arc<Mutex<Preset>>, env: &PageEnv) {
    let Ok(mut current) = preset.lock() else {
        warn!("no se pudo actualizar el preset desde el entorno de la página");
        return;
    };

    if !env.language.is_empty() {
        current.location.language = env.language.clone();
    }
    if !env.browser_language.is_empty() {
        current.location.browser_language = env.browser_language.clone();
    }
    if !env.tz_name.is_empty() {
        current.location.tz_name = env.tz_name.clone();
    }
    if !env.region.is_empty() {
        current.location.region = env.region.clone();
    }
    if env.screen_width != 0 {
        current.screen.width = env.screen_width;
    }
    if env.screen_height != 0 {
        current.screen.height = env.screen_height;
    }
}

/// Clasifica la respuesta ya repetida desde Rust.
///
/// El punto delicado: un 200 con cuerpo vacío o con `push_server` vacío es **rechazo**,
/// no un error transitorio (`docs/06-risks-and-ops.md` §2).
fn build_outcome(
    status: u16,
    url: &str,
    protobuf: Vec<u8>,
    cookies: CookieJar,
    user_agent: &str,
) -> SignOutcome {
    if status != 200 {
        return SignOutcome::Rejected(RejectReason::HttpStatus(status));
    }
    if protobuf.is_empty() {
        return SignOutcome::Rejected(RejectReason::EmptyBody);
    }

    match FetchResult::decode(&protobuf) {
        Ok(result) => {
            if let Some(reason) = result.rejection_reason() {
                return SignOutcome::Rejected(reason);
            }
        }
        Err(e) => return SignError::Decode(e.to_string()).into(),
    }

    SignOutcome::Ok(SignedFetch {
        protobuf,
        cookies,
        user_agent: user_agent.to_string(),
        signed_url: url.to_string(),
    })
}

/// Valor de un parámetro de la query, ya decodificado en lo que necesitamos.
fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| {
            v.replace("%2B", "+")
                .replace("%2F", "/")
                .replace("%3D", "=")
        })
}

/// Percent-encoding de un valor de query. Los valores de firma llevan `/`, `+` y `=`.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `sec-ch-ua-platform` coherente con el preset. Si no cuadra con el UA, es una
/// incoherencia más de las que TikTok mira.
fn platform_hint(preset: &Preset) -> &'static str {
    match preset.device.os.as_str() {
        "windows" => "\"Windows\"",
        "mac" => "\"macOS\"",
        _ => "\"Linux\"",
    }
}

/// Cliente HTTP para repetir la petición firmada.
///
/// Sin redirecciones automáticas: un 3xx aquí sería TikTok mandándonos a otro sitio, y
/// seguirlo enmascararía el rechazo.
fn http_client(user_agent: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(user_agent.to_string())
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .expect("cliente HTTP")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::proto::Writer;

    fn valid_protobuf() -> Vec<u8> {
        let mut w = Writer::new();
        w.str_field(2, "cursor")
            .map_entry(7, "wss_push_room_id", "1")
            .str_field(
                10,
                "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            );
        w.finish()
    }

    #[test]
    fn query_param_is_extracted_and_decoded() {
        let url = "https://x/?a=1&msToken=ab%2Bcd%3D&z=2";
        assert_eq!(query_param(url, "msToken").as_deref(), Some("ab+cd="));
        assert_eq!(query_param(url, "a").as_deref(), Some("1"));
        assert_eq!(query_param(url, "falta"), None);
        assert_eq!(query_param("https://x/sin-query", "a"), None);
    }

    #[test]
    fn non_200_is_a_rejection() {
        let outcome = build_outcome(403, "", vec![1], CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::HttpStatus(403))
        ));
    }

    #[test]
    fn empty_body_on_200_is_a_rejection_not_a_transport_error() {
        let outcome = build_outcome(200, "", Vec::new(), CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyBody)
        ));
        assert!(!outcome.is_retryable());
    }

    #[test]
    fn empty_push_server_on_200_is_a_rejection() {
        let body = Writer::new().str_field(2, "cursor").clone().finish();
        let outcome = build_outcome(200, "", body, CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyPushServer)
        ));
    }

    #[test]
    fn a_valid_response_carries_context_for_the_websocket() {
        let outcome = build_outcome(
            200,
            "https://webcast.tiktok.com/webcast/im/fetch/?X-Gnarly=K",
            valid_protobuf(),
            CookieJar::parse("msToken=abc; ttwid=xyz"),
            "UA-de-prueba",
        );
        let signed = outcome.ok().expect("debería ser una firma válida");
        // El WS necesita exactamente estas cookies y este UA, no otros.
        assert_eq!(signed.cookies.get("ttwid"), Some("xyz"));
        assert_eq!(signed.user_agent, "UA-de-prueba");
        assert!(signed.signed_url.contains("X-Gnarly"));
    }
}
