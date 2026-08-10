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

  var sleep = function (ms) {
    return new Promise(function (r) { setTimeout(r, ms); });
  };

  // Firma sin leer la respuesta (Plan B de docs/01-architecture.md §D2).
  //
  // `webcast.tiktok.com/webcast/im/fetch/` **no** devuelve cabeceras CORS, así que desde
  // la página el cuerpo es ilegible: `fetch` resuelve a `undefined` y un `fetch` pristino
  // desde un iframe da "Load failed". Verificado en F2 contra un directo real.
  //
  // Lo que sí ocurre es que la petición *sale firmada*: webmssdk le añade X-Bogus,
  // X-Gnarly, X-Dynosaur y msToken. Esa URL aparece en el Performance Timeline aunque la
  // respuesta no se pueda leer, así que el puente devuelve la URL firmada y es Rust quien
  // la repite con su propio cliente HTTP, donde no hay CORS que valga.
  window.__ttlSign = async function (req) {
    try {
      // Solo miramos las entradas nuevas: una firma anterior dejaría su URL caducada aquí.
      var offset = performance.getEntriesByType("resource").length;

      // `no-cors` evita el error de consola; la petición sale igual y se firma igual.
      try {
        await fetch(req.url, {
          method: "GET",
          credentials: "include",
          mode: "no-cors",
        });
      } catch (e) {
        // Se espera que falle al leer: lo que importa es que haya salido.
      }

      var signed = null;
      for (var i = 0; i < 30 && !signed; i++) {
        var entries = performance.getEntriesByType("resource");
        for (var j = entries.length - 1; j >= offset; j--) {
          if (entries[j].name.indexOf("/webcast/im/fetch/") !== -1) {
            signed = entries[j].name;
            break;
          }
        }
        if (!signed) {
          await sleep(100);
        }
      }

      if (!signed) {
        post({
          type: "error",
          request_id: req.request_id,
          message: "la petición no llegó a salir: sin entrada de im/fetch en el timeline",
        });
        return;
      }

      post({
        type: "signed",
        request_id: req.request_id,
        url: signed,
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
