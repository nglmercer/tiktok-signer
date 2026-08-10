# 02 — Roadmap

Seis fases. Cada una tiene un criterio de aceptación binario: o pasa o no se avanza.

El orden no es negociable en un punto concreto: **F1 va antes que el webview**. Validar
el modelo de conexión con una petición copiada a mano cuesta una hora; descubrir que el
modelo estaba mal con un motor de webview de por medio cuesta días de depuración en el
sitio equivocado.

---

## F0 — Reconocimiento

**Sin código.** Capturar la verdad de hoy.

1. Abrir una live cualquiera en un navegador real, con DevTools en la pestaña Network.
2. Localizar la petición a `webcast.tiktok.com/webcast/im/fetch/`.
3. Guardar como fixture en `fixtures/f0/`:
   - URL completa con todos los query params (`Copy as cURL`).
   - Todas las cabeceras de request, incluida `Cookie` y `User-Agent`.
   - El cuerpo de la respuesta en binario (`im_fetch.pb`).
   - La URL del WebSocket que abre después (pestaña WS).
4. Anotar la versión de `webmssdk` que sirve TikTok hoy (buscar `webmssdk` en Sources)
   en `fixtures/f0/NOTES.md`.

**Aceptación:** existe `fixtures/f0/` con la cURL, el `.pb` y la URL del WS.

> Los fixtures contienen cookies de sesión. `fixtures/` va en `.gitignore` salvo
> `NOTES.md`.

---

## F1 — Validar el modelo sin webview

Replicar a mano lo capturado, con `reqwest` + `tokio-tungstenite`, en un único binario
de ejemplo (`crates/ttl-live-ws/examples/replay.rs`, ya escrito: parsea el `Copy as
cURL`, repite la petición, decodifica el protobuf, construye la URI y conecta).

- [ ] Reproducir la petición del fixture tal cual → 200 con cuerpo protobuf no vacío.
- [ ] Extraer `push_server`, `route_params`, `cursor`, `internal_ext`.
- [ ] Construir la URI del WS según [05](05-spec-websocket-client.md), incluido el
      `&version_code=270000` duplicado.
- [ ] Conectar el WS con `Cookie` + `User-Agent` del paso anterior.
- [ ] Recibir al menos un frame `msg`.

**Aceptación:** llegan frames por el WebSocket usando parámetros capturados a mano.

**Si falla aquí**, el problema está en el modelo de conexión y no en la firma. Revisar
[00 — Investigación](00-research.md) antes de seguir.

> Ojo: los parámetros firmados caducan en ~30 s. Si F1 falla por caducidad y no por
> otra cosa, el síntoma es un rechazo del WS, no de la petición HTTP.

---

## F2 — Motor webview

Primera vez que aparece `wry`. Ver [04](04-spec-webview-bridge.md) para el contrato.

- [ ] `tao::EventLoop` en el hilo principal, tokio en un hilo aparte (D3 de
      [01](01-architecture.md)).
- [ ] Ventana invisible, navegación a `https://www.tiktok.com/@<user>/live`.
- [ ] `with_initialization_script`: instalar el puente **antes** de que corra webmssdk.
- [ ] Readiness gate: no aceptar peticiones hasta que `window.byted_acrawler` exista,
      con timeout y error tipado si no aparece.
- [ ] `with_ipc_handler` + correlación por `request_id`.
- [ ] `Signer::fetch()` async devolviendo `SignedFetch`.

**Aceptación:** `Signer::fetch(room_id)` devuelve bytes protobuf con `push_server` no
vacío, **sin** usar nada del fixture de F0.

---

## F3 — Integración y servidor HTTP

- [ ] Cookies del webview → `CookieJar` → cabecera `X-Set-TT-Cookie`.
- [ ] `GET /webcast/fetch` según [03](03-spec-sign-server.md).
- [ ] Mapeo de errores a códigos HTTP, incluido 429.

**Aceptación:** un cliente TikTokLive de terceros (Python, apuntando
`WebDefaults.tiktok_sign_url` a este servidor) conecta y recibe eventos. Es la
validación cruzada más barata que existe: si un cliente que no hemos escrito nosotros
funciona, el contrato está bien.

---

## F4 — Cliente WebSocket propio

- [ ] `ttl-live-ws` con la construcción de URI de F1, ya no ad-hoc.
- [ ] Heartbeat cada ~10 s.
- [ ] `ack` con `internal_ext`.
- [ ] `ping_interval`/`ping_timeout` deshabilitados (TikTok no responde pong).
- [ ] Sin reconexión interna; cierre expuesto al orquestador.
- [ ] Distinguir el handshake rechazado con status **200** como "detectado", no como
      transitorio.

**Aceptación:** sesión estable > 10 minutos con heartbeat y acks correctos.

---

## F5 — Robustez y operación

- [ ] Pool de webviews con reciclado; una sesión de cookies por instancia.
- [ ] Watchdog: recargar la página si `byted_acrawler` desaparece o si N firmas
      consecutivas salen rechazadas.
- [ ] Métricas: tasa de rechazo, latencia de firma, edad de la sesión.
- [ ] `GET /healthz` con el estado del pool.
- [ ] *Opcional:* fast-path nativo de X-Gnarly, con el webview como oráculo de
      validación en tests (comparar firma nativa vs firma del SDK sobre la misma entrada).

**Aceptación:** 24 h en marcha sin intervención manual.

---

## Fuera de alcance

- Parseo del protobuf y modelado de eventos (ya existe).
- Sesión autenticada (`sessionid`) y plataforma `mobile` — **decisión pendiente**, ver
  [06 §Decisiones abiertas](06-risks-and-ops.md#decisiones-abiertas).
- Endpoint `/webcast/sign_url` (opcional en la spec de Euler, innecesario para el WS).
