// Puente JS↔Rust. Se inyecta con `with_initialization_script`, es decir **antes** de
// que corra webmssdk.js, para que cuando el SDK parchee `fetch` nosotros ya estemos
// instalados y no observemos un estado a medias.
//
// Contrato completo en docs/04-spec-webview-bridge.md.
(function () {
  if (window.__ttlBridge) return;
  window.__ttlBridge = true;

  var post = function (o) {
    try {
      window.ipc.postMessage(JSON.stringify(o));
    } catch (e) {
      // Sin IPC no hay nada que hacer y no queremos romper la página.
    }
  };

  // El spread sobre un Uint8Array grande desborda la pila: se trocea.
  var b64 = function (buf) {
    var bytes = new Uint8Array(buf);
    var s = "";
    var CHUNK = 0x8000;
    for (var i = 0; i < bytes.length; i += CHUNK) {
      s += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
    }
    return btoa(s);
  };

  // Guardamos la URL real que ve `fetch` tras el parcheo del SDK. `res.url` puede no
  // reflejar los parámetros que añade webmssdk; esto es lo que mantiene abierta la
  // puerta al Plan B (docs/01-architecture.md §D2).
  var lastRequestUrl = null;
  var nativeFetch = window.fetch;
  window.fetch = function (input, init) {
    try {
      var url = typeof input === "string" ? input : (input && input.url) || null;
      if (url && url.indexOf("/webcast/im/fetch/") !== -1) {
        lastRequestUrl = url;
      }
    } catch (e) {}
    return nativeFetch.apply(this, arguments);
  };

  window.__ttlSign = async function (req) {
    lastRequestUrl = null;
    try {
      // `fetch` ya está parcheado por webmssdk: añade X-Bogus / X-Gnarly por su cuenta.
      var res = await fetch(req.url, {
        method: "GET",
        credentials: "include",
        headers: { Accept: "application/protobuf" },
      });
      var buf = await res.arrayBuffer();
      post({
        type: "result",
        request_id: req.request_id,
        status: res.status,
        url: lastRequestUrl || res.url,
        body_b64: b64(buf),
        cookie: document.cookie,
      });
    } catch (e) {
      post({ type: "error", request_id: req.request_id, message: String(e) });
    }
  };

  // Readiness gate: sin `byted_acrawler` las firmas salen sin X-Gnarly y TikTok las
  // rechaza, con un síntoma que apunta al sitio equivocado.
  var t0 = Date.now();
  var poll = setInterval(function () {
    if (typeof window.byted_acrawler !== "undefined") {
      clearInterval(poll);
      var version = null;
      try {
        version = window.byted_acrawler.version || null;
      } catch (e) {}
      post({ type: "ready", sdk_version: version });
    } else if (Date.now() - t0 > 30000) {
      clearInterval(poll);
      post({ type: "error", request_id: 0, message: "sdk_not_ready" });
    }
  }, 100);
})();
