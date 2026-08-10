# Fixtures — notas de captura

Plantilla para F0. El resto del contenido de `fixtures/` **no se versiona**: contiene
cookies de sesión reales.

## Captura YYYY-MM-DD

- **Sala:** `@usuario` / `room_id=`
- **Versión de webmssdk:** (buscar `webmssdk` en la pestaña Sources de DevTools)
- **User-Agent:**
- **Archivos:**
  - `f0/im_fetch.curl` — `Copy as cURL` de la petición a `/webcast/im/fetch/`
  - `f0/im_fetch.pb` — cuerpo binario de la respuesta
  - `f0/ws_url.txt` — URL del WebSocket que abre después

### Observaciones

- ¿Aparece `X-Bogus`, `X-Gnarly` o ambos en la query?
- ¿Qué cookies viajan en la petición? ¿Cuáles son `HttpOnly`?
  (relevante para [04 — Cookies](../docs/04-spec-webview-bridge.md#cookies))
- ¿Coinciden `browser_name`/`browser_version` de la query con el `User-Agent`?

## Captura 2026-08-10 (F0, automatizada)

Ya no hace falta capturar a mano con DevTools: `fetch-dump` hace la captura entera desde
el propio motor, con sesión autenticada.

```sh
cargo run -p ttl-sign-webview --example login
cargo run -p ttl-sign-webview --example fetch-dump -- <usuario> fixtures/f0/im_fetch.pb
```

- **Versión de webmssdk:** `window.byted_acrawler.version` es `undefined` en el build
  actual; el objeto expone `frontierSign`, `registerWsSigner`, `init`, `report`,
  `setTTWebid`, `setTTWebidV2`, `setTTWid`, `setUserMode`, `getReferer`, `isWebmssdk`.

### Observaciones

- **Firmas en la query:** `X-Gnarly` (~332 chars), `X-Dynosaur` (~392 chars),
  `X-Bogus=1` (hoy es un relleno de un carácter) y `msToken`.
- **Cookies:** el cookie manager de WebKit devuelve `ttwid`, `odin_tt`, `tt_csrf_token`,
  `tt_chain_token`, `csrfToken`, `msToken` y, con sesión, `sessionid`, `sessionid_ss`,
  `sid_tt`, `sid_guard`, `uid_tt`, `uid_tt_ss`, `tt-target-idc`. `document.cookie` solo ve
  un subconjunto.
- **Números de campo del protobuf, confirmados** contra la respuesta real:
  `2 = cursor`, `5 = internal_ext`, `7 = route_params` (map), `10 = push_server`.
  Los campos `8` (heartbeat_duration) y `9` (need_ack) **no aparecen**; el `1` se repite
  (~13 veces) con los mensajes ya incluidos en la respuesta.
- **`route_params` solo trae `wrss` e `imprp`** (este último vacío). `cursor` e
  `internal_ext` los pone el cliente en la query del WebSocket.
- **`push_server`** = `wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/`,
  no el `webcast<N>-ws-web-<idc>` que describe `docs/00`.
- **La URI del WebSocket va firmada** (`X-Gnarly`). Ver `docs/05`.
