# 03 — Spec: sign server HTTP

Contrato compatible con clientes TikTokLive existentes (Python y Node), para poder
validar la implementación contra software que no hemos escrito nosotros.

Base URL configurable. En el cliente Python se apunta con:

```python
WebDefaults.tiktok_sign_url = "http://localhost:8080"
```

---

## `GET /webcast/fetch`

**Obligatorio.** Es el único endpoint que hace falta para conectar el WebSocket.

### Query params de entrada

Los clientes envían el set completo de parámetros de navegador. El servidor puede
ignorar los que quiera y regenerarlos desde su propio `DevicePreset`, **excepto**:

| Param | Uso |
|---|---|
| `room_id` | Obligatorio. Sala objetivo. |
| `user_agent` | Si viene, el preset del servidor debe ser coherente con él, o se ignora y se devuelve el UA realmente usado. |
| `client` / `client_query` | Identificador del cliente. Solo para logs. |
| `session_id`, `tt_target_idc` | Solo si se soporta sesión autenticada (fuera de alcance por ahora). |

> **Regla:** o el servidor respeta el `user_agent` entrante **y** genera params
> coherentes con él, o impone el suyo **y** lo comunica de vuelta. Lo que no puede
> pasar es firmar con un UA y que el cliente abra el WS con otro. Ver
> [00 §3](00-research.md#3-coherencia-ua--parámetros-causa-nº1-de-rechazo).

### Respuesta 200

- **Body:** los bytes protobuf de TikTok, **sin envolver, sin transformar**.
  `Content-Type: application/protobuf`.
- **Header `X-Set-TT-Cookie`** (obligatorio): cookie-string con las cookies usadas al
  firmar. Formato `SimpleCookie`, p. ej.:

  ```
  X-Set-TT-Cookie: msToken=xxxx; tt-target-idc=useast1a; ttwid=yyyy
  ```

  Si falta, el cliente Python aborta con `EMPTY_COOKIES` antes de intentar el WS.

- **Header `X-Set-TT-User-Agent`** (extensión propia, recomendada): el UA realmente
  usado, para que el cliente lo replique en el WS.

Headers de diagnóstico que el cliente Python registra si están presentes (opcionales,
baratos de emitir y útiles en soporte): `X-Agent-Id`, `X-Request-Id`, `X-Log-Code`.

### Errores

| Código | Cuándo | Body |
|---|---|---|
| 400 | `room_id` ausente o no numérico | JSON `{"message": "..."}` |
| 429 | Límite de tasa propio alcanzado | JSON `{"message": "...", "limit_label": "..."}` — el cliente lo usa para el mensaje de rate limit |
| 502 | TikTok devolvió cuerpo vacío o `push_server` vacío (= detectados) | JSON `{"message": "..."}` |
| 503 | El pool de webviews no está listo | JSON `{"message": "..."}` |

**Nunca** devolver 200 con cuerpo vacío: el cliente lo interpreta como
`EMPTY_PAYLOAD` / "detectado por TikTok" y el mensaje de error resultante apunta al
sitio equivocado.

---

## `GET /webcast/sign_url`

**Opcional, no implementado.** Firma genérica de otros endpoints de TikTok LIVE. No
interviene en la conexión al WebSocket. Si algún día hace falta: recibe una URL, la
devuelve con los parámetros de firma añadidos.

---

## `GET /healthz`

Extensión propia (F5).

```json
{
  "ready": true,
  "webviews": [
    { "id": 0, "sdk_ready": true, "session_age_s": 412, "signs": 37, "rejects": 0 }
  ]
}
```

---

## Notas de implementación

- El body de respuesta es binario: cuidado con middlewares de compresión o de logging
  que asuman UTF-8.
- El endpoint es efectivamente **stateful** respecto del pool: la respuesta y las
  cookies deben venir de la **misma** instancia de webview. No se pueden servir bytes de
  una instancia con cookies de otra.
- Latencia esperada: el round-trip real dentro del webview domina. Presupuestar
  timeout de cliente ≥ 15 s y recordar que el resultado caduca a los ~30 s, así que
  colas largas hacen caducar firmas ya emitidas — mejor rechazar con 429 que encolar.
