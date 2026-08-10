# 01 — Arquitectura

## Principio rector

> No reimplementamos el algoritmo de firma. Hacemos que la propia página de TikTok
> firme y ejecute la petición, y nos llevamos los bytes.

Todo lo demás sale de ahí.

## Layout de crates

```
tiktok-signer/
├── Cargo.toml                  # workspace
└── crates/
    ├── ttl-sign-core/          # sin I/O, sin webview: tipos y construcción de requests
    ├── ttl-sign-webview/       # motor wry: event loop, pool, IPC
    ├── ttl-sign-server/        # axum: expone el core por HTTP (compat Euler)
    └── ttl-live-ws/            # cliente WebSocket sobre el resultado firmado
```

### `ttl-sign-core`

Sin dependencias de red ni de GUI. Testeable en CI sin display.

- `DevicePreset` / `LocationPreset` / `ScreenPreset`: **única fuente de verdad** para
  UA + parámetros de navegador. Un preset genera el UA *y* los params; no existe camino
  para desincronizarlos (ver [00 §3](00-research.md#3-coherencia-ua--parámetros-causa-nº1-de-rechazo)).
- `FetchParams::build(room_id, preset) -> String` — la query de `/webcast/im/fetch/`.
- `WsParams::build(route_params, preset) -> String` — la query del WebSocket, incluido
  el `&version_code=270000` duplicado del final.
- `CookieJar` mínimo: serializar/deserializar cookie-strings estilo `X-Set-TT-Cookie`.
- `SignOutcome`: distingue explícitamente
  `Ok(bytes)` / `Rejected` (cuerpo vacío o `push_server` vacío = detectados) /
  `Transport(err)`. **Nunca** colapsar rechazo y error de red en el mismo tipo.

### `ttl-sign-webview`

- `wry = "0.56"`, `tao = "0.36"`.
- API pública async: `Signer::fetch(room_id, preset) -> Result<SignedFetch>`, donde
  `SignedFetch { protobuf: Bytes, cookies: CookieJar, user_agent: String }`.
- Detalle del contrato JS↔Rust en [04 — Spec: puente webview](04-spec-webview-bridge.md).

### `ttl-sign-server`

- `axum`. Endpoints en [03 — Spec: sign server](03-spec-sign-server.md).
- Capa fina: traduce HTTP ↔ llamadas al `Signer`. Sin lógica propia.

### `ttl-live-ws`

- `tokio-tungstenite`. Detalles en [05](05-spec-websocket-client.md).
- Consume `SignedFetch` y expone un stream de frames crudos. El parseo de protobuf es
  del consumidor, no de este crate.

## Decisiones

### D1 — Librería primero, servidor después

El núcleo es una librería embebible; el servidor HTTP es un binario opcional encima.

*Por qué:* el caso de uso inmediato es un cliente Rust propio, que no necesita dar un
salto por HTTP para hablar consigo mismo. Pero exponer el contrato de Euler permite que
clientes existentes (Python/Node) apunten a este signer sin cambios, lo que además es
la forma más barata de **validar** la implementación contra un cliente de terceros.

### D2 — La petición se hace *dentro* del webview (Plan A)

Tres opciones evaluadas:

| Plan | Cómo | Ventaja | Coste |
|---|---|---|---|
| **A** ✅ | `fetch()` dentro de la página; el `arrayBuffer` vuelve en base64 por IPC | Cero reimplementación; msToken y cookies reales automáticos; sobrevive a updates de webmssdk | Todo el tráfico pasa por el webview; el body cruza el IPC en base64 |
| B | Interceptar el `fetch` parcheado, capturar la URL ya firmada, reproducirla con `reqwest` | Permite proxy/IP por conexión y control total del cliente HTTP | Hay que replicar el cookie jar a mano; ventana de caducidad más ajustada |
| C | Llamar a `window.byted_acrawler.frontierSign(...)` directamente | Más rápido, sin round-trip de red en el webview | Los símbolos cambian en cada build del SDK |

**Implementado B**, no A. La nota sobre CORS que había aquí resultó ser falsa al
contrastarla contra un directo real en F2 (2026-08-10):

- La página **ya no** pide `/webcast/im/fetch/`. El reproductor web actual usa
  `/webcast/room/enter/`, `/webcast/room/check_alive/` y `/webcast/feed/`.
- Un `fetch` de `/webcast/im/fetch/` desde la página resuelve a `undefined` con el
  `fetch` parcheado por webmssdk, y a `TypeError: Load failed` con un `fetch` pristino
  sacado de un iframe. El cuerpo no se puede leer desde el navegador.
- Lo que **sí** funciona es que la petición *sale firmada*: webmssdk le pone `X-Bogus`,
  `X-Gnarly` (332 chars), `X-Dynosaur` (392 chars) y `msToken`, y la URL resultante
  aparece en el Performance Timeline aunque la respuesta sea ilegible.

De ahí el diseño actual: la página **firma** y Rust **repite** la petición con su propio
cliente HTTP, donde no hay CORS. `X-Dynosaur` es un parámetro de firma que no estaba en
[00](00-research.md) §2 y que aparece hoy junto a `X-Gnarly`.

**C** sigue descartado.

### D3 — El event loop del webview manda en el hilo principal

`wry`/`tao` **exigen** que el event loop viva en el hilo principal. Por tanto:

```
main thread                          worker runtime (tokio)
───────────                          ──────────────────────
tao::EventLoop::run()   ◄── proxy ── Signer::fetch()  (async)
  └─ WebView(s)                        └─ espera oneshot::Receiver
       └─ ipc_handler ──────────────►  oneshot::Sender
```

- `EventLoopProxy<SignRequest>` para inyectar trabajo desde tokio.
- `SignRequest` lleva un `oneshot::Sender<SignOutcome>` dentro.
- El runtime de tokio se construye a mano (`Runtime::new()`) y se lanza en un hilo
  aparte; `#[tokio::main]` en `main()` **no** sirve.

Consecuencia: el binario servidor no puede ser un `#[tokio::main]` normal. Queda
documentado aquí porque es el error de integración más probable.

### D4 — Una sesión de cookies por webview

Cada instancia de webview mantiene su propio cookie jar (msToken, `tt-target-idc`).
Mezclar salas en una sola sesión es lo que dispara los límites de tasa. El pool arranca
con 1 instancia; escalar es añadir instancias, no compartir la existente.

### D5 — Sin reconexión del WebSocket

Los parámetros firmados caducan en ~30 s. Reconectar = repetir el flujo desde el paso 2.
El crate `ttl-live-ws` **no** implementa reintento interno; expone el cierre y deja la
decisión al orquestador.

## Plataforma

Linux / WebKitGTK. Requiere display: la ventana se crea con `with_visible(false)`, pero
sigue necesitando X11 o Wayland. Sin display, `Xvfb`. Variables útiles ante fallos de
render en entornos sin GPU:

```
WEBKIT_DISABLE_DMABUF_RENDERER=1
WEBKIT_DISABLE_COMPOSITING_MODE=1
```
