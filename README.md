# tiktok-signer

Custom sign server para TikTok LIVE, escrito desde cero en Rust, usando un webview
(`wry`) como motor de firma.

**Objetivo único:** obtener una respuesta protobuf válida de
`https://webcast.tiktok.com/webcast/im/fetch/` para poder abrir el WebSocket de la sala.
El parseo del protobuf y el consumo de eventos **ya existen** y quedan fuera de alcance.

## Estado

Fase actual: **F0 — Reconocimiento** (ver [roadmap](docs/02-roadmap.md)).

El workspace ya está montado con los cuatro crates y las herramientas de F1–F3
implementadas contra las specs. Lo que **falta** para avanzar es el fixture de F0: sin
una captura real no se puede validar nada de lo que sigue.

| Crate | Estado |
|---|---|
| `ttl-sign-core` | Presets, queries, cookie jar, `SignOutcome` y lectura mínima de protobuf. Con tests, sin I/O. |
| `ttl-sign-webview` | Motor `wry`: puente JS, readiness gate, navegación, descubrimiento de canales y firma. Verificado contra TikTok hasta la firma incluida. |
| `ttl-live-ws` | Cliente WS con heartbeat, `ack` y rechazo 200 tipado. **Sin verificar contra TikTok** (bloqueado por lo de abajo). |
| `ttl-sign-server` | `GET /webcast/fetch` y `GET /healthz` según la spec de Euler. |

> Los números de campo del protobuf están puestos según el esquema de los clientes de
> referencia y **hay que confirmarlos contra `fixtures/f0/im_fetch.pb`** en F1; están
> documentados en un solo sitio (`crates/ttl-sign-core/src/proto.rs`) para que corregirlos
> sea cambiar cuatro constantes.

### Bloqueo actual: `/webcast/im/fetch/` responde 200 con cuerpo vacío

Verificado contra directos reales el 2026-08-10. Lo que **sí** funciona:

- descubrir quién está en directo y resolver `unique_id` → `room_id` (sin firma),
- que webmssdk firme nuestra URL: sale con `X-Gnarly`, `X-Dynosaur`, `X-Bogus` y `msToken`,
- repetir esa URL desde Rust con las cookies del webview (incluidas las `HttpOnly`).

Lo que no: la respuesta llega **200 con 0 bytes**, que es el patrón de rechazo silencioso.
Da igual el juego de parámetros (probado también el de la propia página), las cabeceras
(`Referer`, `Origin`, `Sec-Fetch-*`) o quién repita la petición: el navegador tampoco
puede leerla. Y el reproductor web actual **ya no llama a ese endpoint** — usa
`/webcast/room/enter/`, `/webcast/room/check_alive/` y `/webcast/feed/`.

Hipótesis pendientes, por orden:

1. El endpoint exige sesión autenticada (`sessionid`) — es la decisión abierta de
   [06](docs/06-risks-and-ops.md#decisiones-abiertas), ahora en el camino crítico.
2. El flujo cambió y la URL del WebSocket sale hoy de `/webcast/room/enter/`, no de
   `/webcast/im/fetch/`. Eso invalidaría [00 §1](docs/00-research.md#1-flujo-real-de-conexión).

Para diagnosticar hay `cargo run -p ttl-sign-webview --example page-probe -- <usuario> "<js>"`,
que evalúa JS en la página y enseña qué pide de verdad.

## Uso

```sh
cargo test --workspace          # no necesita display

# quién está en directo, y unique_id → room_id (sin firma, sin display)
cargo run -p ttl-live-ws --example rooms -- usuario1 usuario2

# flujo completo contra un canal real: descubrir → room_id → firmar → WebSocket
cargo run -p ttl-sign-webview --example live-check          # elige canal solo
cargo run -p ttl-sign-webview --example live-check -- usuario

# F1 — validar el modelo con el fixture capturado a mano
cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl

# F3 — sign server (necesita display; sin él, Xvfb)
TTL_BIND=127.0.0.1:8080 cargo run -p ttl-sign-server
```

Linux/WebKitGTK: la ventana es invisible pero sigue haciendo falta X11 o Wayland. En
entornos sin GPU, `WEBKIT_DISABLE_DMABUF_RENDERER=1` y
`WEBKIT_DISABLE_COMPOSITING_MODE=1`.

## Documentación

| Documento | Contenido |
|---|---|
| [00 — Investigación](docs/00-research.md) | Flujo real de conexión, qué se firma y qué no, spec de Euler |
| [01 — Arquitectura](docs/01-architecture.md) | Crates, modelo de hilos, decisiones de diseño |
| [02 — Roadmap](docs/02-roadmap.md) | Fases F0–F5, entregables y criterios de aceptación |
| [03 — Spec: sign server](docs/03-spec-sign-server.md) | Endpoints HTTP, compatibilidad con clientes existentes |
| [04 — Spec: puente webview](docs/04-spec-webview-bridge.md) | Contrato IPC JS↔Rust, script de inicialización |
| [05 — Spec: cliente WebSocket](docs/05-spec-websocket-client.md) | Construcción de la URI, headers, heartbeat, ack |
| [06 — Riesgos y operación](docs/06-risks-and-ops.md) | Modos de fallo, detección, límites, mantenimiento |

## Resumen en tres líneas

1. Solo hay **una** firma en el camino crítico: la petición HTTP `/webcast/im/fetch/`.
2. La URL del WebSocket viene **ya firmada por TikTok** dentro de esa respuesta protobuf.
3. El webview no se usa para reimplementar el algoritmo, sino para que la propia
   página de TikTok firme y ejecute la petición por nosotros.

## Referencias

- [Euler Stream — Custom Sign Servers](https://www.eulerstream.com/docs/sign-server/custom-sign-servers)
- [isaackogan/TikTokLive](https://github.com/isaackogan/TikTokLive) — cliente Python de referencia
- [zerodytrash/TikTok-Live-Connector](https://github.com/zerodytrash/TikTok-Live-Connector) — cliente Node de referencia
- [carcabot/tiktok-xgnarly-decoded](https://github.com/carcabot/tiktok-xgnarly-decoded) — reversing de X-Gnarly (webmssdk 5.1.3-ZTCA)
- [carcabot/tiktok-signature](https://github.com/carcabot/tiktok-signature) — enfoque headless-browser en Node
