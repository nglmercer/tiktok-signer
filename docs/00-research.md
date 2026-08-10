# 00 — Investigación

Fuente: documentación de Euler Stream y lectura del código de `isaackogan/TikTokLive`
(rama `master`, agosto 2026). Todo lo afirmado aquí está contrastado contra el código
del cliente, no contra suposiciones.

> **Revisión del 2026-08-10 (F2).** Lo verificado contra directos reales cambia dos cosas
> de este documento:
>
> 1. **El paso 2 exige sesión autenticada.** En anónimo, `/webcast/im/fetch/` devuelve 200
>    con cuerpo vacío y `/webcast/room/enter/` devuelve `User doesn't login`. Ver
>    [06 §Decisiones abiertas](06-risks-and-ops.md#decisiones-abiertas).
> 2. **Hay una firma más que las de §2:** `X-Dynosaur` (~392 chars), que acompaña hoy a
>    `X-Gnarly`. Como firma el SDK, no hay que implementarla, pero conviene no
>    sorprenderse al verla.
>
> Además, el reproductor web actual **no llama a `/webcast/im/fetch/`**: usa
> `/webcast/room/enter/`, `/webcast/room/check_alive/` y `/webcast/feed/`. Queda por
> confirmar, ya con sesión, si `im/fetch` sigue siendo el camino al WebSocket o si hoy
> sale de `room/enter`.

## 1. Flujo real de conexión

```
┌─ 1 ─────────────────────────────────────────────────────────────┐
│ unique_id → room_id                                             │
│ GET https://www.tiktok.com/@<user>/live  (scrape del HTML)      │
│ SIN FIRMA                                                       │
└─────────────────────────────────────────────────────────────────┘
                                │
┌─ 2 ─────────────────────────── ▼ ───────────────────────────────┐
│ GET https://webcast.tiktok.com/webcast/im/fetch/                │
│     ?room_id=..&X-Bogus=..&X-Gnarly=..&msToken=..&<~30 params>  │
│ Header: User-Agent coherente con los params del navegador       │
│ Cookies: msToken, tt-target-idc, ...                            │
│ ◄── ESTE ES EL ÚNICO PUNTO QUE HAY QUE FIRMAR                   │
│ → 200 application/protobuf: ProtoMessageFetchResult             │
└─────────────────────────────────────────────────────────────────┘
                                │
┌─ 3 ─────────────────────────── ▼ ───────────────────────────────┐
│ Del protobuf se extrae:                                         │
│   push_server   → wss://webcast<N>-ws-web-<idc>.tiktok.com/...  │
│   route_params  → map<string,string> YA FIRMADO POR TIKTOK      │
│   cursor, internal_ext                                          │
└─────────────────────────────────────────────────────────────────┘
                                │
┌─ 4 ─────────────────────────── ▼ ───────────────────────────────┐
│ WS URI = push_server + "?" + route_params + ws_client_params    │
│                      + "&version_code=270000"                   │
│ Header Cookie: las MISMAS cookies del paso 2                    │
│ Header User-Agent: el MISMO UA del paso 2                       │
│ SIN FIRMA PROPIA                                                │
└─────────────────────────────────────────────────────────────────┘
```

### Consecuencia de diseño

El "custom signer" no firma URLs de WebSocket. Es un componente capaz de emitir **una**
petición HTTP GET que TikTok acepte. Todo lo demás es transporte.

Esto reduce el alcance de forma drástica y descarta de entrada cualquier diseño que
intente firmar la URL del WS por su cuenta.

## 2. Qué son X-Bogus, X-Gnarly y msToken

| Parámetro | Qué es | Origen |
|---|---|---|
| `X-Bogus` | Firma legacy, tipo AES, sobre la query string | `webmssdk.js` |
| `X-Gnarly` | Sucesor de X-Bogus. Blob base64 (~332 chars, alfabeto custom, magic byte `K`) con payload TLV de 16 campos: MD5 de query/body/UA, timestamps, versión de SDK, enteros aleatorios, cabecera XOR; cifrado con un ChaCha-like usando 48 bytes aleatorios embebidos en el propio ciphertext a un offset derivado de checksum | `webmssdk.js` |
| `msToken` | Token anti-replay emitido por el servidor | Cookie, puesta al cargar tiktok.com / reporte del mssdk |

`webmssdk.js` **parchea `window.fetch` y `XMLHttpRequest.prototype`** para añadir estos
parámetros de forma transparente a las peticiones salientes. Esa es la palanca que
usamos: no llamamos a funciones internas, dejamos que la página haga la petición.

Versiones observadas del SDK: de `1.0.0.211` hasta `2.0.0.485-ZTCA`. La versión concreta
que sirve TikTok cambia sin aviso; el alfabeto base64, el magic byte y el core ChaCha se
han mantenido estables, lo que evoluciona es el conjunto de campos TLV y la derivación de
rondas. Por eso una reimplementación nativa es mantenimiento perpetuo y el webview no.

## 3. Coherencia UA ↔ parámetros (causa nº1 de rechazo)

De la documentación de Euler, textualmente sobre `browser_name` y `browser_version`:
si el `User-Agent` no coincide con esos parámetros, TikTok rechaza la petición.

Parámetros que deben derivarse de una **única** fuente de verdad (un `DevicePreset`):

- `browser_name`, `browser_version`, `browser_platform`, `os`
- cabecera `User-Agent`
- (secundarios, coherentes entre sí) `screen_width`/`screen_height`, `tz_name`,
  `app_language`/`browser_language`/`webcast_language`, `region`/`priority_region`

## 4. Contrato del sign server según Euler

### `GET /webcast/fetch` — obligatorio

Proxy hacia `/webcast/im/fetch` de TikTok. Query params documentados:

```
aid=1988                     app_language=en           app_name=tiktok_web
browser_language=en-US       browser_online=true       cookie_enabled=true
cursor=                      debug=false               device_platform=web
did_rule=3                   fetch_rule=1              history_comment_count=6
history_comment_cursor=      identity=audience         internal_ext=
last_rtt=0                   live_id=12                resp_content_type=protobuf
screen_height=1920           screen_width=1080         sup_ws_ds_opt=1
tz_name=UTC                  version_code=270000       notice=CUSTOM_SIGN_SERVER
device_id=<19 dígitos>       room_id=<target>          contact_us=<email>
X-Bogus=<firma>              msToken=<firma>
browser_name=<...>           browser_version=<...>
```

Cabecera obligatoria: `User-Agent` coherente con `browser_name`/`browser_version`.

Respuesta: **los bytes protobuf tal cual** vienen de TikTok. Cita de la documentación:
"Just return the Protobuf response from `/webcast/fetch`, and all libraries will work
with your custom server".

### `GET /webcast/sign_url` — opcional

Firma genérica de otros endpoints de TikTok LIVE. No es necesario para conectar el WS.

### Detalle no documentado pero obligatorio: `X-Set-TT-Cookie`

El cliente Python exige que la respuesta del sign server incluya la cabecera
`X-Set-TT-Cookie` con las cookies (formato cookie-string) que usó al firmar. Si falta,
aborta con `EMPTY_COOKIES`. Esas cookies son las que después viajan en el header `Cookie`
del WebSocket. Sin ellas el WS se rechaza.

## 5. Parámetros del WebSocket

Del cliente de referencia, el set fijo que se añade a los `route_params`:

```
aid=1988                app_language=<lang>      app_name=tiktok_web
browser_platform=<..>   browser_language=<..>    browser_name=<..>
browser_version=<..>    browser_online=true      cookie_enabled=true
tz_name=<..>            device_platform=web      identity=audience
live_id=12              sup_ws_ds_opt=1          update_version_code=2.0.0
version_code=180800     client_enter=1           ws_direct=1
did_rule=3              webcast_language=<lang>  screen_height=<..>
screen_width=<..>       heartbeat_duration=10000 resp_content_type=protobuf
history_comment_count=6 last_rtt=<100..200>
```

Y después, **anexado a mano al final de la query string**:

```
&version_code=270000
```

Sí, `version_code` aparece dos veces con valores distintos. Es lo que hace producción y
un `HashMap` no lo puede representar, de ahí que el cliente de referencia lo concatene
como string aparte. Hay que replicarlo.

## 6. Caducidad y reconexión

Las URLs firmadas caducan en **~30 segundos**. El cliente de referencia deshabilita
explícitamente todo mecanismo de reintento del WebSocket por este motivo: reconectar
significa rehacer el paso 2 completo, no reusar la URI.

Además, un `InvalidStatusCode` con código **200** en el handshake del WS no es un
transitorio, es "TikTok te ha detectado"; reintentar es inútil. Si el WS se firmó con un
`sessionid` y no se envía la cookie `sessionid`, el rechazo es por "illegal secret key".

## 7. Formato de los frames del WebSocket

Fuera de alcance (ya resuelto en el cliente existente), pero relevante para F1 y F4:

- Cada mensaje es un `WebcastPushFrame`.
- Solo los de `payload_type == "msg"` llevan eventos; `hb`, `ack`, `im_enter_room_resp`
  son de transporte y se descartan.
- La compresión se indica en `headers[].key == "compress_type"`; si vale `gzip` hay que
  descomprimir antes de parsear.
- El `ack` se responde con un `WebcastPushFrame{payload_type:"ack", payload_encoding:"pb",
  log_id: <del frame recibido>, payload: internal_ext || "-"}`.
