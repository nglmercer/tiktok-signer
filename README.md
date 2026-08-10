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
| `ttl-sign-webview` | Motor `wry`: puente JS, readiness gate, navegación, descubrimiento de canales, firma de URLs arbitrarias y replay. Verificado contra TikTok: firma y replay funcionan (`room/info` responde 25 KB). |
| `ttl-live-ws` | Cliente WS con heartbeat, `ack` y rechazo 200 tipado. Handshake verificado; la recepción de frames, no. |
| `ttl-sign-server` | `GET /webcast/fetch` y `GET /healthz` según la spec de Euler. |

> Los números de campo del protobuf ya están **confirmados** contra una respuesta real
> (`2 = cursor`, `5 = internal_ext`, `7 = route_params`, `10 = push_server`); ver
> `fixtures/NOTES.md`. Captura reproducible con
> `cargo run -p ttl-sign-webview --example fetch-dump`.

### Estado real del flujo (medido el 2026-08-10)

| Paso | Estado |
|---|---|
| 1. Descubrir quién está en directo | funciona (DOM de `/live`, sin firma) |
| 1b. `unique_id` → `room_id` + estado | funciona (sin firma, sin display) |
| 2. `/webcast/im/fetch/` firmado | **funciona con sesión** (~25–44 KB de protobuf) |
| 3. Decodificar el protobuf | funciona; números de campo confirmados contra la respuesta real |
| 4. Handshake del WebSocket | se acepta (101, `handshake-msg: OK`) |
| 5. Recibir frames | **no llega ninguno** |

Dos cosas que la documentación daba por buenas resultaron falsas al medirlas, y las dos
salieron caras en tiempo de depuración:

1. **En anónimo no hay flujo.** `/webcast/room/enter/` responde `User doesn't login` y
   `/webcast/im/fetch/` responde 200 con **0 bytes** — el mismo rechazo dicho en voz baja.
   Por la misma ruta de firma, `room/info` devuelve 25 KB, así que no fallaban ni la firma
   ni el replay. De ahí el flujo de login de abajo.
2. **La URI del WebSocket lleva firma propia** (`X-Gnarly`), al contrario de lo que decía
   [00 §1](docs/00-research.md#1-flujo-real-de-conexión). Sin ella el servidor **acepta el
   handshake y luego calla**, que es el peor síntoma posible: todo parece correcto.
   Implementado en `Signer::sign_ws_uri`, **sin verificar todavía** que con eso lleguen
   frames: el límite de tasa cortó las pruebas (docs/06 §5 — firmar mucho seguido desde
   una IP lo despierta; conviene esperar antes de insistir).

Herramientas para seguir desde aquí, todas contra salas reales:

```sh
cargo run -p ttl-sign-webview --example endpoint-probe -- <usuario>  # compara endpoints
cargo run -p ttl-sign-webview --example fetch-dump -- <usuario>      # estructura del protobuf
cargo run -p ttl-sign-webview --example ws-probe -- <usuario>        # todo lo que entra y sale del socket
cargo run -p ttl-sign-webview --example page-probe -- <usuario> "<js>"  # qué hace la página
```

### Iniciar sesión

```sh
cargo run -p ttl-sign-webview --example login                 # 5 min de plazo
cargo run -p ttl-sign-webview --example login -- --timeout 600
cargo run -p ttl-sign-webview --example login -- --logout     # borra la sesión
```

Abre una ventana **visible** con la página de login de TikTok, espera a que termines
(sondeando la cookie `sessionid` cada 2 s) y guarda la sesión en
`$XDG_CONFIG_HOME/ttl-signer/session` con permisos `0600`. Si se agota el plazo no guarda
nada. `live-check` y el servidor la recogen solos; `TTL_SESSION_ID` tiene prioridad y
`TTL_SESSION_FILE` cambia la ruta.

Esa cookie **es** la cuenta: quien la tenga es tú para TikTok. Vive fuera del repositorio
y solo la lee tu usuario. Los logs la redactan salvo los 8 primeros caracteres.

Queda sin verificar el tramo final (protobuf → WebSocket → frames): necesita una cuenta.
Con `sessionid`, el WS además **exige** esa cookie en el handshake o responde
"illegal secret key".

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
