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
