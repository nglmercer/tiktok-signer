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

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use base64::Engine as _;
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

use crate::ipc::{FromPage, ToPage, ToPageText};

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
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            preset: Preset::default(),
            landing_url: "https://www.tiktok.com/live".into(),
            sign_timeout: Duration::from_secs(15),
            sdk_ready_timeout: Duration::from_secs(30),
            contact_us: String::new(),
        }
    }
}

/// Trabajo inyectado en el event loop desde el runtime de tokio.
enum UserEvent {
    Sign {
        params: Box<FetchParams>,
        reply: oneshot::Sender<SignOutcome>,
    },
    /// Paso 1 sin firma. `url: None` pide el DOM ya renderizado.
    Text {
        url: Option<String>,
        reply: oneshot::Sender<Result<String, SignError>>,
    },
    /// Despierta el event loop desde el handler de IPC.
    ///
    /// Hace falta porque el handler corre fuera del closure del event loop: cuando marca
    /// la instancia como lista, nadie vacía la cola de peticiones hasta que llega *algún*
    /// evento. Sin esto, todo lo encolado antes de `ready` se queda ahí hasta que caduca.
    Wake,
    Shutdown,
}

/// Handle async del motor. Clonable y `Send`: es lo que ve el servidor HTTP.
#[derive(Clone)]
pub struct Signer {
    proxy: EventLoopProxy<UserEvent>,
    config: EngineConfig,
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
    /// `internal_ext`, `device_id`).
    pub async fn fetch_with(&self, params: FetchParams) -> SignOutcome {
        let (tx, rx) = oneshot::channel();
        if self
            .proxy
            .send_event(UserEvent::Sign {
                params: Box::new(params),
                reply: tx,
            })
            .is_err()
        {
            return SignError::EngineGone("el event loop se ha cerrado".into()).into();
        }

        match tokio::time::timeout(self.config.sign_timeout, rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => SignError::EngineGone("el motor descartó la petición".into()).into(),
            Err(_) => SignError::Timeout(self.config.sign_timeout.as_millis() as u64).into(),
        }
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
            Ok(Err(_)) => Err(SignError::EngineGone("el motor descartó la petición".into())),
            Err(_) => Err(SignError::Timeout(
                self.config.sign_timeout.as_millis() as u64
            )),
        }
    }

    /// El preset con el que firma este motor. El WebSocket tiene que usar este mismo UA.
    pub fn preset(&self) -> &Preset {
        &self.config.preset
    }

    /// Pide el cierre ordenado del event loop.
    pub fn shutdown(&self) {
        let _ = self.proxy.send_event(UserEvent::Shutdown);
    }
}

/// Estado compartido entre el event loop y el handler de IPC. Ambos corren en el hilo
/// principal, así que `Rc<RefCell<_>>` es suficiente y evita locks.
#[derive(Default)]
struct Shared {
    next_id: u64,
    /// Peticiones en vuelo, por `request_id`.
    pending: HashMap<u64, oneshot::Sender<SignOutcome>>,
    /// Peticiones de texto en vuelo (paso 1, sin firma).
    pending_text: HashMap<u64, oneshot::Sender<Result<String, SignError>>>,
    /// Peticiones recibidas antes de que el SDK estuviese listo.
    queued: Vec<(Box<FetchParams>, oneshot::Sender<SignOutcome>)>,
    /// Igual, para las de texto: `ready` implica además que el documento ya cargó, que
    /// es lo que hace falta para que el DOM tenga algo que leer.
    queued_text: Vec<(Option<String>, oneshot::Sender<Result<String, SignError>>)>,
    ready: bool,
    sdk_version: Option<String>,
    signs: u64,
    rejects: u64,
}

impl Shared {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn resolve(&mut self, request_id: u64, outcome: SignOutcome) {
        if outcome.is_rejected() {
            self.rejects += 1;
        }
        match self.pending.remove(&request_id) {
            Some(tx) => {
                let _ = tx.send(outcome);
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
            let _ = tx.send(make_error().into());
        }
        for (_, tx) in self.queued.drain(..) {
            let _ = tx.send(make_error().into());
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
        .with_title("ttl-sign-webview")
        .with_visible(false)
        .build(&event_loop)
        .expect("no se pudo crear la ventana (¿hay display? si no, usa Xvfb)");

    let shared = Rc::new(RefCell::new(Shared::default()));
    let user_agent = config.preset.user_agent();

    let ipc_shared = Rc::clone(&shared);
    let ipc_ua = user_agent.clone();
    let ipc_proxy = proxy.clone();
    // El webview aún no existe cuando se construye el handler, y el handler necesita
    // leer el cookie manager: se comparte por una celda que se rellena justo después.
    let webview_slot: Rc<RefCell<Option<Rc<wry::WebView>>>> = Rc::new(RefCell::new(None));
    let ipc_slot = Rc::clone(&webview_slot);

    let builder = WebViewBuilder::new()
        .with_url(&config.landing_url)
        .with_user_agent(&user_agent)
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
            handle_page_message(&ipc_shared, msg, &ipc_ua, jar_from_webview);
            // El handler no es el event loop: hay que despertarlo para que drene la cola.
            let _ = ipc_proxy.send_event(UserEvent::Wake);
        });

    let webview = build_webview(builder, &window).expect("no se pudo crear el webview");

    let webview = Rc::new(webview);
    *webview_slot.borrow_mut() = Some(Rc::clone(&webview));

    std::thread::spawn({
        let signer = Signer {
            proxy,
            config: config.clone(),
        };
        move || worker(signer)
    });

    let started = Instant::now();
    let sdk_ready_timeout = config.sdk_ready_timeout;

    event_loop.run(move |event, _target, control_flow| {
        // Wait despierta con cada evento; el deadline del SDK se comprueba en cada paso.
        *control_flow = ControlFlow::Wait;

        {
            let mut state = shared.borrow_mut();
            let waiting = !state.queued.is_empty() || !state.queued_text.is_empty();
            if !state.ready && waiting && started.elapsed() > sdk_ready_timeout {
                error!("el SDK no apareció en {sdk_ready_timeout:?}");
                state.fail_all(|| SignError::SdkNotReady);
            }
        }

        match event {
            Event::UserEvent(UserEvent::Sign { params, reply }) => {
                let mut state = shared.borrow_mut();
                if !state.ready {
                    debug!("petición en cola: el SDK todavía no está listo");
                    state.queued.push((params, reply));
                    return;
                }
                let id = state.next_id();
                state.pending.insert(id, reply);
                state.signs += 1;
                drop(state);
                dispatch(&webview, &shared, id, &params, &config.preset);
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

        // El puente puede haber emitido `ready` desde el handler de IPC mientras había
        // peticiones en cola; es aquí donde se drenan.
        let to_dispatch: Vec<(u64, Box<FetchParams>)> = {
            let mut state = shared.borrow_mut();
            if !state.ready || state.queued.is_empty() {
                Vec::new()
            } else {
                let queued: Vec<_> = state.queued.drain(..).collect();
                let mut out = Vec::with_capacity(queued.len());
                for (params, reply) in queued {
                    let id = state.next_id();
                    state.pending.insert(id, reply);
                    state.signs += 1;
                    out.push((id, params));
                }
                out
            }
        };
        for (id, params) in to_dispatch {
            dispatch(&webview, &shared, id, &params, &config.preset);
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
fn dispatch(
    webview: &wry::WebView,
    shared: &Rc<RefCell<Shared>>,
    request_id: u64,
    params: &FetchParams,
    preset: &Preset,
) {
    let msg = ToPage {
        request_id,
        url: params.url(preset),
    };
    if let Err(e) = webview.evaluate_script(&msg.to_script()) {
        error!(request_id, error = %e, "no se pudo inyectar la firma");
        shared
            .borrow_mut()
            .resolve(request_id, SignError::EngineGone(e.to_string()).into());
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
) {
    match msg {
        FromPage::Ready { sdk_version } => {
            let mut state = shared.borrow_mut();
            state.ready = true;
            state.sdk_version = sdk_version.clone();
            info!(sdk_version = ?sdk_version, "puente listo");
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
                    state.resolve(request_id, SignError::Bridge(message).into());
                }
            }
        }
        FromPage::Result {
            request_id,
            status,
            url,
            body_b64,
            cookie,
        } => {
            let outcome = build_outcome(
                status,
                &url,
                &body_b64,
                &cookie,
                user_agent,
                jar_from_webview,
            );
            shared.borrow_mut().resolve(request_id, outcome);
        }
    }
}

/// Clasifica la respuesta. El punto delicado: un 200 con cuerpo vacío o con
/// `push_server` vacío es **rechazo**, no un error transitorio
/// (`docs/06-risks-and-ops.md` §2).
fn build_outcome(
    status: u16,
    url: &str,
    body_b64: &str,
    document_cookie: &str,
    user_agent: &str,
    mut cookies: CookieJar,
) -> SignOutcome {
    if status != 200 {
        return SignOutcome::Rejected(RejectReason::HttpStatus(status));
    }

    let protobuf = match base64::engine::general_purpose::STANDARD.decode(body_b64) {
        Ok(bytes) => bytes,
        Err(e) => return SignError::Decode(format!("base64 inválido: {e}")).into(),
    };
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

    // Las del cookie manager mandan; `document.cookie` solo añade lo que falte.
    let from_document = CookieJar::parse(document_cookie);
    let mut merged = from_document;
    merged.merge(&cookies);
    std::mem::swap(&mut cookies, &mut merged);

    SignOutcome::Ok(SignedFetch {
        protobuf,
        cookies,
        user_agent: user_agent.to_string(),
        signed_url: url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::proto::Writer;

    fn valid_protobuf() -> String {
        let mut w = Writer::new();
        w.str_field(2, "cursor")
            .map_entry(7, "wss_push_room_id", "1")
            .str_field(
                10,
                "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            );
        base64::engine::general_purpose::STANDARD.encode(w.finish())
    }

    #[test]
    fn non_200_is_a_rejection() {
        let outcome = build_outcome(403, "", "", "", "UA", CookieJar::new());
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::HttpStatus(403))
        ));
    }

    #[test]
    fn empty_body_on_200_is_a_rejection_not_a_transport_error() {
        let outcome = build_outcome(200, "", "", "", "UA", CookieJar::new());
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyBody)
        ));
        assert!(!outcome.is_retryable());
    }

    #[test]
    fn empty_push_server_on_200_is_a_rejection() {
        let body = base64::engine::general_purpose::STANDARD
            .encode(Writer::new().str_field(2, "cursor").clone().finish());
        let outcome = build_outcome(200, "", &body, "", "UA", CookieJar::new());
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyPushServer)
        ));
    }

    #[test]
    fn bad_base64_is_transport_not_rejection() {
        let outcome = build_outcome(200, "", "no-es-base64!!", "", "UA", CookieJar::new());
        assert!(outcome.is_retryable());
        assert!(!outcome.is_rejected());
    }

    #[test]
    fn webview_cookies_win_over_document_cookie() {
        let jar = CookieJar::parse("msToken=del-manager; ttwid=solo-httponly");
        let outcome = build_outcome(
            200,
            "https://webcast.tiktok.com/webcast/im/fetch/?X-Gnarly=K",
            &valid_protobuf(),
            "msToken=del-documento; otra=1",
            "UA-de-prueba",
            jar,
        );
        let signed = outcome.ok().expect("debería ser una firma válida");
        assert_eq!(signed.cookies.get("msToken"), Some("del-manager"));
        assert_eq!(signed.cookies.get("ttwid"), Some("solo-httponly"));
        assert_eq!(signed.cookies.get("otra"), Some("1"));
        assert_eq!(signed.user_agent, "UA-de-prueba");
        assert!(signed.signed_url.contains("X-Gnarly"));
    }
}
