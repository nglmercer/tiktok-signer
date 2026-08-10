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

  // URL final de la petición, ya con X-Bogus / X-Gnarly puestos por el SDK. Es lo que
  // mantiene abierta la puerta al Plan B (docs/01-architecture.md §D2).
  //
  // Se lee del Performance Timeline y **no** parcheando `window.fetch`: envolver `fetch`
  // desde el script de inicialización rompe la cadena que instala webmssdk encima, y la
  // llamada acaba resolviendo a `undefined` en vez de a una `Response`. Verificado en F2.
  var signedUrlFor = function (needle) {
    try {
      var entries = performance.getEntriesByType("resource");
      for (var i = entries.length - 1; i >= 0; i--) {
        if (entries[i].name.indexOf(needle) !== -1) {
          return entries[i].name;
        }
      }
    } catch (e) {}
    return null;
  };

  // `XMLHttpRequest.prototype` también lo parchea webmssdk (docs/00-research.md §2), y a
  // diferencia de `fetch` devuelve algo utilizable en esta página: el `fetch` de
  // `www.tiktok.com/live` resuelve a `undefined` para esta petición. Verificado en F2.
  var xhrArrayBuffer = function (url) {
    return new Promise(function (resolve, reject) {
      var xhr = new XMLHttpRequest();
      xhr.open("GET", url, true);
      xhr.responseType = "arraybuffer";
      xhr.withCredentials = true;
      xhr.setRequestHeader("Accept", "application/protobuf");
      xhr.onload = function () {
        resolve({
          status: xhr.status,
          url: xhr.responseURL || url,
          buffer: xhr.response,
        });
      };
      xhr.onerror = function () {
        reject(new Error("XHR error status=" + xhr.status));
      };
      xhr.send();
    });
  };

  window.__ttlSign = async function (req) {
    try {
      // El `fetch` de la página está parcheado por webmssdk y añade X-Bogus / X-Gnarly.
      // Si no devuelve una `Response` utilizable, se cae a XHR, que el SDK parchea
      // igual: lo que no se puede hacer es componer la firma por nuestra cuenta.
      var res = null;
      try {
        res = await fetch(req.url, {
          method: "GET",
          credentials: "include",
          headers: { Accept: "application/protobuf" },
        });
      } catch (e) {
        res = null;
      }

      var status, buffer, finalUrl;
      if (res && typeof res.arrayBuffer === "function") {
        status = res.status;
        buffer = await res.arrayBuffer();
        finalUrl = res.url;
      } else {
        var out = await xhrArrayBuffer(req.url);
        status = out.status;
        buffer = out.buffer;
        finalUrl = out.url;
      }

      post({
        type: "result",
        request_id: req.request_id,
        status: status,
        url: signedUrlFor("/webcast/im/fetch/") || finalUrl,
        body_b64: b64(buffer),
        cookie: document.cookie,
      });
    } catch (e) {
      post({ type: "error", request_id: req.request_id, message: String(e) });
    }
  };

  // Paso 1 del flujo (docs/00-research.md §1): no lleva firma, pero se hace desde la
  // página igualmente para reutilizar la sesión y el UA reales.
  //
  // - `req.url`  → GET de texto (el lookup uniqueId → roomId).
  // - sin `url`  → el DOM ya renderizado, que es la única forma de ver quién está en
  //   directo: la página /live no trae esos datos en el HTML, los pinta el cliente.
  window.__ttlText = async function (req) {
    try {
      // `js:<expresión>` evalúa en la página y devuelve el resultado como texto. Es la
      // vía de diagnóstico del puente (qué pide la página, qué símbolos hay); no
      // interviene en ninguna firma.
      if (req.url && req.url.indexOf("js:") === 0) {
        var value = eval(req.url.slice(3));
        if (value && typeof value.then === "function") {
          value = await value;
        }
        post({
          type: "text",
          request_id: req.request_id,
          status: 200,
          body: typeof value === "string" ? value : JSON.stringify(value),
        });
        return;
      }
      if (!req.url) {
        post({
          type: "text",
          request_id: req.request_id,
          status: 200,
          body: document.documentElement.outerHTML,
        });
        return;
      }
      var res = await fetch(req.url, {
        method: "GET",
        credentials: "include",
        headers: { Accept: "application/json, text/html" },
      });
      post({
        type: "text",
        request_id: req.request_id,
        status: res.status,
        body: await res.text(),
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
