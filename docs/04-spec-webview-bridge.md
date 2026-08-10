# 04 — Spec: puente webview

Contrato entre el motor Rust (`ttl-sign-webview`) y el JavaScript inyectado en la
página de TikTok.

Implementa el **Plan A** de [01 §D2](01-architecture.md#d2--la-petición-se-hace-dentro-del-webview-plan-a):
la petición HTTP se ejecuta dentro de la página, con el `fetch` ya parcheado por
`webmssdk.js`, y los bytes vuelven a Rust por IPC.

---

## Ciclo de vida

```
1. tao::EventLoop en el hilo principal
2. WindowBuilder::new().with_visible(false)
3. WebViewBuilder
     .with_initialization_script(BRIDGE_JS)   ← antes de que corra webmssdk
     .with_ipc_handler(...)
     .with_url("https://www.tiktok.com/@<user>/live")
4. El puente hace polling de window.byted_acrawler → emite `{"type":"ready"}`
5. Rust marca la instancia como disponible
6. Por cada firma: evaluate_script(`__ttlSign(<json>)`) → IPC `{"type":"result",...}`
```

`with_initialization_script` corre en cada documento **antes** que los scripts de la
página. Es importante: si el puente se inyectase después, webmssdk ya habría parcheado
`fetch` y estaríamos observando un estado que puede haber cambiado.

## Readiness gate

No aceptar peticiones hasta ver `ready`. Sin esto, las primeras firmas salen sin
`X-Gnarly` y TikTok las rechaza, con un síntoma ("detectado") que no apunta a la causa
real (carrera de arranque).

- Polling cada 100 ms sobre `typeof window.byted_acrawler !== "undefined"`.
- Timeout de 30 s → error tipado `SdkNotReady`, no un timeout genérico.
- Si tras `ready` el símbolo desaparece (renavegación, SPA), volver a estado no-listo y
  recargar (watchdog de F5).

---

## Mensajes IPC

Dirección Rust → JS: `evaluate_script("__ttlSign(<json>)")`.
Dirección JS → Rust: `window.ipc.postMessage(<json string>)`.

Todos los mensajes llevan `request_id` (u64) salvo `ready`. Rust correlaciona con un
`HashMap<u64, oneshot::Sender<SignOutcome>>`.

### JS → Rust

```jsonc
// El SDK está cargado y el puente instalado
{ "type": "ready", "sdk_version": "1.0.0.368" }

// Petición completada
{
  "type": "result",
  "request_id": 42,
  "status": 200,
  "url": "https://webcast.tiktok.com/webcast/im/fetch/?...&X-Gnarly=K...",  // URL final ya firmada
  "body_b64": "CgoK...",       // arrayBuffer de la respuesta en base64
  "cookie": "msToken=...; tt-target-idc=useast1a"   // document.cookie
}

// Fallo dentro de la página
{ "type": "error", "request_id": 42, "message": "TypeError: Failed to fetch" }
```

### Rust → JS

```jsonc
{
  "request_id": 42,
  "url": "https://webcast.tiktok.com/webcast/im/fetch/?<query construida en Rust>"
}
```

La query la construye **Rust** (`ttl-sign-core::FetchParams`). El JS no compone
parámetros: solo pasa la URL al `fetch` parcheado y deja que webmssdk añada `X-Bogus`
y `X-Gnarly`.

---

## Boceto del script inyectado

```js
(function () {
  if (window.__ttlBridge) return;
  window.__ttlBridge = true;

  const post = (o) => window.ipc.postMessage(JSON.stringify(o));

  const b64 = (buf) => {
    const bytes = new Uint8Array(buf);
    let s = "";
    const CHUNK = 0x8000;                       // evita desbordar el stack en spread
    for (let i = 0; i < bytes.length; i += CHUNK) {
      s += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
    }
    return btoa(s);
  };

  window.__ttlSign = async function (req) {
    try {
      // fetch está parcheado por webmssdk: añade X-Bogus / X-Gnarly por su cuenta
      const res = await fetch(req.url, {
        method: "GET",
        credentials: "include",
        headers: { Accept: "application/protobuf" },
      });
      post({
        type: "result",
        request_id: req.request_id,
        status: res.status,
        url: res.url,
        body_b64: b64(await res.arrayBuffer()),
        cookie: document.cookie,
      });
    } catch (e) {
      post({ type: "error", request_id: req.request_id, message: String(e) });
    }
  };

  const t0 = Date.now();
  const poll = setInterval(() => {
    if (typeof window.byted_acrawler !== "undefined") {
      clearInterval(poll);
      post({ type: "ready", sdk_version: window.byted_acrawler?.version ?? null });
    } else if (Date.now() - t0 > 30000) {
      clearInterval(poll);
      post({ type: "error", request_id: 0, message: "sdk_not_ready" });
    }
  }, 100);
})();
```

Sobre `res.url`: puede no reflejar los parámetros añadidos por el SDK según cómo
parchee. Se emite igualmente porque es lo que habilita el **Plan B** (capturar la URL
firmada y reproducirla con `reqwest`); si resulta que llega sin `X-Gnarly`, hay que
interceptar `window.fetch` en el puente y registrar el argumento real. Verificar esto
en F2 y anotar el resultado aquí.

---

## Modelo de hilos en Rust

```rust
enum UserEvent {
    Sign { room_id: u64, reply: oneshot::Sender<SignOutcome> },
    Shutdown,
}

// hilo principal
let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
let proxy = event_loop.create_proxy();

// hilo worker
std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async { server::run(proxy).await });
});

event_loop.run(move |event, _, control_flow| { /* ... */ });
```

`#[tokio::main]` en `main()` **no** sirve: el event loop tiene que estar en el hilo
principal y `run()` no retorna.

---

## Cookies

`document.cookie` no expone las `HttpOnly`. Si alguna cookie necesaria para el WS
resulta ser `HttpOnly` (candidata: `ttwid`), hay que leerla del cookie manager de
WebKitGTK en lugar de por IPC. **Verificar en F2** comparando las cookies obtenidas por
IPC contra las del fixture de F0, y documentar aquí el resultado.

*Implementación:* el motor no espera a esa verificación — lee **las dos** fuentes y las
fusiona, con las del cookie manager (`WebView::cookies()`, que sí ve las `HttpOnly`)
ganando ante colisión. Lo que queda pendiente de F2 es comprobar si `document.cookie`
aporta algo que el manager no tenga; si no aporta nada, se puede quitar del contrato IPC.

## Notas de plataforma

- Linux → WebKitGTK. Requiere display aunque la ventana sea invisible; si no hay,
  `Xvfb`.
- Ante fallos de render en entornos sin GPU:
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`, `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
- Versiones: `wry = "0.56"`, `tao = "0.36"`.
- Una instancia de webview = una sesión de cookies. No compartir entre salas
  ([01 §D4](01-architecture.md#d4--una-sesión-de-cookies-por-webview)).
