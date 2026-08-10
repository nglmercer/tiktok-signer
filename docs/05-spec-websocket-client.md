# 05 — Spec: cliente WebSocket

Consume un `SignedFetch` y abre la conexión. **No firma nada**: los parámetros de ruta
vienen ya firmados por TikTok dentro de la respuesta protobuf.

---

## Entrada

Del protobuf `ProtoMessageFetchResult` devuelto por `/webcast/im/fetch/`:

| Campo | Uso |
|---|---|
| `push_server` | Base de la URI del WebSocket (`wss://webcast<N>-ws-web-<idc>.tiktok.com/webcast/im/ws/`) |
| `route_params` | `map<string,string>` con los parámetros firmados por TikTok |
| `cursor` | Debe existir. Si falta → abortar, la respuesta no es válida |
| `internal_ext` | Payload de los `ack` |

Más, del `SignedFetch`: el cookie jar y el User-Agent usados al firmar.

## Validación previa

Abortar con error tipado, **antes** de intentar conectar, si:

- `cursor` vacío → `InitialCursorMissing`
- `push_server` vacío → `WebsocketUrlMissing`
- `route_params` vacío → `WebsocketUrlMissing`

Un `push_server` vacío en un 200 significa que TikTok rechazó la petición sin decirlo.

---

## Construcción de la URI

```
uri = push_server
    + "?"
    + join("&", [k=v for (k,v) in route_params if v != ""]     // primero, y solo no vacíos
                 ++ [k=v for (k,v) in ws_client_params])        // después: los nuestros pisan
    + "&version_code=270000"
```

Tres detalles que hay que respetar tal cual:

1. Los `route_params` **vacíos se descartan**.
2. Los `ws_client_params` se aplican **después**, es decir, ganan ante colisión.
3. `&version_code=270000` se **concatena al final como string**, aunque
   `ws_client_params` ya lleve `version_code=180800`. La query lleva `version_code` dos
   veces con valores distintos. Es lo que hace producción; un `HashMap` no lo puede
   representar y por eso va aparte.

### `ws_client_params`

Derivados del mismo `DevicePreset`/`LocationPreset` usado al firmar:

```
aid=1988                     app_language=<lang>
app_name=tiktok_web          browser_platform=<preset>
browser_language=<lang_country>  browser_name=<preset>
browser_version=<preset>     browser_online=true
cookie_enabled=true          tz_name=<location>
device_platform=web          identity=audience
live_id=12                   sup_ws_ds_opt=1
update_version_code=2.0.0    version_code=180800
client_enter=1               ws_direct=1
did_rule=3                   webcast_language=<lang>
screen_height=<preset>       screen_width=<preset>
heartbeat_duration=10000     resp_content_type=protobuf
history_comment_count=6      last_rtt=<aleatorio 100..200>
```

Más, en tiempo de conexión: `room_id=<sala>` y `compress=gzip` (o vacío si se quiere sin
compresión).

## Cabeceras

| Cabecera | Valor |
|---|---|
| `Cookie` | Cookie-string del jar del sign server (`X-Set-TT-Cookie`), todas las cookies |
| `User-Agent` | **Exactamente** el mismo UA usado al firmar |

## Opciones de conexión

- `ping_interval = None`, `ping_timeout = None`. TikTok **no responde pong**; dejar el
  keepalive del protocolo activo cierra la conexión sola.
- El keepalive real es el heartbeat de aplicación (ver abajo).

---

## Errores de handshake

| Síntoma | Significado | Acción |
|---|---|---|
| Status **200** en el handshake | TikTok ha rechazado la conexión (detección). La cabecera `Handshake-Msg` trae el motivo | **No reintentar.** Error tipado `WebcastBlocked200` |
| `Handshake-Msg: illegal secret key` | Se firmó con `sessionid` pero no se envió la cookie `sessionid` | Corregir el jar, no reintentar |
| Cierre inmediato tras conectar | Probable caducidad (>30 s desde la firma) | Rehacer el flujo desde `/webcast/im/fetch/` |

La respuesta del handshake trae `Handshake-Options`, una cadena estilo cookie
(`k=v; k=v`) con opciones del servidor. Parsear y exponer; útil en diagnóstico.

---

## Bucle de mensajes

Cada mensaje binario es un `WebcastPushFrame`.

- Solo `payload_type == "msg"` lleva eventos. `hb`, `ack`, `im_enter_room_resp` y
  cualquier otro son de transporte: descartar (log a DEBUG), no intentar parsear.
- Compresión: en `headers[]`, la entrada con `key == "compress_type"`.
  - ausente o `none` → parsear `payload` directamente
  - `gzip` → descomprimir y luego parsear
  - otro valor → es un cambio de TikTok; loguear a ERROR e intentar parsear crudo

### Ack

Por cada frame `msg` recibido:

```
WebcastPushFrame {
  payload_type: "ack",
  payload_encoding: "pb",
  log_id: <log_id del frame recibido>,
  payload: (internal_ext || "-").as_bytes(),
}
```

### Heartbeat

Cada ~10 s (coherente con `heartbeat_duration=10000`), enviar el `HeartBeatMessage`
correspondiente. Si el envío falla, la conexión está muerta: cerrar y notificar.

---

## Reconexión

**No la hay a nivel de este crate.** Las URIs firmadas caducan en ~30 s, así que no
existe "reconectar": existe rehacer el flujo entero desde el paso 2 de
[00 §1](00-research.md#1-flujo-real-de-conexión).

El crate expone el cierre con su causa y el orquestador decide. Reintentar la misma URI
es siempre inútil, y con un rechazo 200 además es contraproducente.
