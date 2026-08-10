# 06 — Riesgos y operación

## Modos de fallo, por probabilidad

### 1. Desincronización UA ↔ parámetros

**El fallo más frecuente.** Si `browser_name`, `browser_version`, `browser_platform` u
`os` no cuadran con el `User-Agent`, TikTok rechaza. Y rechaza igual que si te hubiera
detectado, así que el síntoma no señala la causa.

*Mitigación:* un único `DevicePreset` genera UA **y** parámetros. No existe API para
fijar un UA suelto. Test unitario que verifique la coherencia de todos los presets.

### 2. "Detectado" confundido con error de red

Un 200 con cuerpo vacío, o con `push_server` vacío, es rechazo. Un handshake de WS que
devuelve status 200 es rechazo. Ninguno de los dos es transitorio y reintentar los
empeora.

*Mitigación:* tipos separados desde el primer día — `Rejected` nunca comparte variante
con `Transport`. Ningún reintento automático sobre `Rejected`.

### 3. Caducidad de ~30 s

Entre firmar y completar el handshake del WS hay 30 segundos. Encolar peticiones bajo
carga produce firmas ya caducadas al llegar a su turno.

*Mitigación:* rechazar con 429 antes que encolar. Medir la latencia firma→handshake y
alertar por encima de 10 s.

### 4. Actualización de webmssdk

TikTok cambia el SDK sin aviso. El conjunto de campos TLV y la derivación de rondas son
las partes que evolucionan.

*Mitigación:* es precisamente la razón de usar webview — el SDK se actualiza solo al
recargar la página. El riesgo real está en el **puente**, si el SDK cambiara la forma de
parchear `fetch`. Watchdog + tasa de rechazo como señal de alarma.

Este riesgo vuelve con toda su fuerza si se implementa el fast-path nativo de F5; por
eso ahí el webview se queda como oráculo de validación en tests.

### 5. Límite de tasa y bloqueo por IP

Firmar es interactuar con un mecanismo anti-bot. Volumen alto desde una IP → bloqueo.
Además va contra los Términos de Servicio de TikTok; con volumen bajo (un cliente
propio) es el uso habitual del ecosistema, pero conviene tenerlo escrito.

*Mitigación:* límite de tasa propio, una sesión de cookies por webview, y no compartir
sesión entre salas.

### 6. Fugas de recursos del webview

Una sesión de WebKitGTK de larga vida acumula memoria y estado.

*Mitigación:* reciclar instancias por edad o por número de firmas. Presupuestar el
reinicio como operación normal, no como recuperación de fallo.

---

## Señales a instrumentar (F5)

| Métrica | Para qué |
|---|---|
| `sign_latency_ms` (p50/p99) | Detectar degradación antes de que cause caducidades |
| `reject_rate` | Señal principal de "el puente ha dejado de funcionar" |
| `session_age_s` por webview | Política de reciclado |
| `sdk_ready` por webview | Detectar renavegaciones que rompen el puente |
| `ws_handshake_200_total` | Contador de detecciones |

Regla de watchdog propuesta: N rechazos consecutivos (arrancar con N=3) → recargar la
página; si tras la recarga sigue, marcar la instancia como no disponible.

---

## Seguridad de los fixtures

`fixtures/` contiene cookies de sesión reales capturadas en F0. Va en `.gitignore`
salvo `NOTES.md`. No pegar cURLs completas en issues ni en logs; los logs que impriman
cookie-strings deben redactar todo salvo los 8 primeros caracteres, como hace el cliente
de referencia con el `sessionid`.

---

## Decisiones abiertas

### ¿Sesión autenticada (`sessionid`)?

Enviar un `sessionid` al signer habilita la plataforma `mobile` y eventos que no llegan
de forma anónima, a cambio de:

- exponer la sesión de una cuenta real al componente que firma,
- el WS pasa a exigir la cookie `sessionid` en el header (si no, "illegal secret key"),
- riesgo sobre la cuenta.

**Estado: resuelta por los hechos el 2026-08-10.** Ya no es una decisión de diseño: el
anónimo **no funciona**. Medido contra directos reales, con la misma ruta de firma para
todos los endpoints:

| Endpoint | Anónimo |
|---|---|
| `/webcast/room/info/` | 200, ~25–30 KB de JSON |
| `/webcast/room/check_alive/` | 200, JSON correcto |
| `/webcast/room/enter/` | 200, `{"message":"User doesn't login"}` |
| `/webcast/im/fetch/` | 200, **0 bytes** |

Que `room/info` responda por esa misma ruta descarta que el problema sea la firma, las
cabeceras o el replay. El cuerpo vacío de `im/fetch` es el mismo "necesitas sesión" que
`room/enter` dice con todas las letras.

Reproducible con `cargo run -p ttl-sign-webview --example endpoint-probe -- <usuario>`.

Consecuencias, que siguen siendo las de antes:

- se expone la sesión de una cuenta real al componente que firma,
- el WS pasa a exigir la cookie `sessionid` en el header (si no, "illegal secret key"),
- riesgo sobre la cuenta.

La diferencia es que ya no se puede elegir no pagarlas. Implementado como
`EngineConfig::session_id` (`TTL_SESSION_ID` en el servidor), vacío por defecto: el
componente no toca ninguna cuenta salvo que se le dé una explícitamente.

### ¿Proxy por sala?

Solo relevante al escalar. Requiere pasar del Plan A al Plan B (reproducir la URL
firmada con `reqwest` a través del proxy). Ver
[01 §D2](01-architecture.md#d2--la-petición-se-hace-dentro-del-webview-plan-a).

**Estado:** no ahora. La spec del puente ya emite `res.url` para no cerrarse esa puerta.
