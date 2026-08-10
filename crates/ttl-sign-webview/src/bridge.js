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

  // --- Sesión ----------------------------------------------------------------------
  //
  // Las cookies se instalan **aquí dentro**, no con `WebView::set_cookie`.
  //
  // Medido el 2026-08-10 en WebKitGTK 2.52: `set_cookie` escribe en un almacén que la
  // página no lee. Las cookies se leen de vuelta desde Rust —parecen instaladas— pero no
  // aparecen en `document.cookie` ni viajan en las peticiones, así que TikTok renderiza
  // la sesión como anónima y `/webcast/im/fetch/` responde 200 con cero bytes. Ese era el
  // "rechazo silencioso": no había rechazo, había sesión que nunca llegó.
  //
  // Este script corre en el origen correcto y antes que nada de la página.
  // `document.cookie` no puede marcar `HttpOnly`, y da igual: al servidor solo le llega
  // la cabecera `Cookie`, que es idéntica.
  var sessionInstalled = 0;
  try {
    if (/(^|\.)tiktok\.com$/.test(location.hostname)) {
      var pairs = window.__ttlSession || [];
      for (var i = 0; i < pairs.length; i++) {
        document.cookie =
          pairs[i][0] + "=" + pairs[i][1] + "; path=/; domain=.tiktok.com; secure";
        sessionInstalled++;
      }
    }
  } catch (e) {}

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

  // URLs de WebSocket que abre la propia página. Envolver el constructor es seguro:
  // webmssdk parchea `fetch` y `XMLHttpRequest`, no esto. Sirve para comparar la URI que
  // construimos nosotros con la que usa el reproductor real.
  //
  // `window.__ttlBlockWs = true` además impide que la página llegue a conectar: devuelve
  // un socket inerte, sin hacer ninguna petición de red. Hace falta porque dos conexiones
  // a la misma sala con la misma sesión se estorban — el servidor acepta la segunda y no
  // le manda nada.
  window.__ttlWsUrls = [];
  // El initialization script puede pedir que el socket del reproductor se bloquee. No lo
  // reseteamos aquí: este archivo corre después de ese script y el valor debe sobrevivir
  // hasta que el reproductor intente conectarse.
  window.__ttlBlockWs = window.__ttlBlockWs === true;
  var NativeWebSocket = window.WebSocket;
  var blockedEvent = function (type, options) {
    var event = null;
    try {
      if (type === "close" && window.CloseEvent) {
        event = new window.CloseEvent(type, options || {});
      } else if (window.Event) {
        event = new window.Event(type);
      }
    } catch (e) {}
    if (!event) event = { type: type };
    if (options) {
      try { event.code = options.code; } catch (e) {}
      try { event.reason = options.reason; } catch (e) {}
      try { event.wasClean = options.wasClean; } catch (e) {}
    }
    return event;
  };
  var BlockedWebSocket = function (url) {
    this.url = String(url);
    this.protocol = "";
    this.readyState = 0;
    this.bufferedAmount = 0;
    this.extensions = "";
    this.binaryType = "blob";
    this.onopen = null;
    this.onmessage = null;
    this.onerror = null;
    this.onclose = null;
    this.__ttlListeners = {};

    var self = this;
    // Let the page install its handlers before reporting the intentionally blocked
    // connection. No WebKit network loader is created by this object.
    setTimeout(function () {
      if (self.readyState !== 0) return;
      self.readyState = 3;
      self.__ttlDispatch("error", blockedEvent("error"));
      self.__ttlDispatch(
        "close",
        blockedEvent("close", { code: 1006, reason: "blocked", wasClean: false })
      );
    }, 0);
  };
  BlockedWebSocket.prototype.__ttlDispatch = function (type, event) {
    event = event || blockedEvent(type);
    try { event.target = this; } catch (e) {}
    try { event.currentTarget = this; } catch (e) {}

    var handler = this["on" + type];
    if (typeof handler === "function") {
      try { handler.call(this, event); } catch (e) {}
    }
    var listeners = this.__ttlListeners[type] || [];
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i].call(this, event); } catch (e) {}
    }
  };
  BlockedWebSocket.prototype.addEventListener = function (type, listener) {
    if (typeof listener !== "function") return;
    var listeners = this.__ttlListeners[type] || (this.__ttlListeners[type] = []);
    for (var i = 0; i < listeners.length; i++) {
      if (listeners[i] === listener) return;
    }
    listeners.push(listener);
  };
  BlockedWebSocket.prototype.removeEventListener = function (type, listener) {
    var listeners = this.__ttlListeners[type] || [];
    for (var i = listeners.length - 1; i >= 0; i--) {
      if (listeners[i] === listener) listeners.splice(i, 1);
    }
  };
  BlockedWebSocket.prototype.dispatchEvent = function (event) {
    if (!event || !event.type) return false;
    this.__ttlDispatch(event.type, event);
    return true;
  };
  BlockedWebSocket.prototype.send = function () {
    if (this.readyState !== 1) {
      var error = new Error("WebSocket is not open");
      error.name = "InvalidStateError";
      throw error;
    }
  };
  BlockedWebSocket.prototype.close = function (code, reason) {
    if (this.readyState === 2 || this.readyState === 3) return;
    this.readyState = 2;
    var self = this;
    setTimeout(function () {
      if (self.readyState === 3) return;
      self.readyState = 3;
      self.__ttlDispatch(
        "close",
        blockedEvent("close", {
          code: typeof code === "number" ? code : 1000,
          reason: reason == null ? "" : String(reason),
          wasClean: true,
        })
      );
    }, 0);
  };
  BlockedWebSocket.CONNECTING = 0;
  BlockedWebSocket.OPEN = 1;
  BlockedWebSocket.CLOSING = 2;
  BlockedWebSocket.CLOSED = 3;
  var WrappedWebSocket = function (url, protocols) {
    try {
      window.__ttlWsUrls.push(String(url));
    } catch (e) {}
    if (window.__ttlBlockWs) {
      // Interceptar es preferible a lanzar: una excepción aquí rompería el reproductor
      // en un sitio que no espera fallos. El stub también evita errores del loader de
      // WebKit que producía el antiguo socket deliberadamente inválido.
      return new BlockedWebSocket(url, protocols);
    }
    return protocols === undefined
      ? new NativeWebSocket(url)
      : new NativeWebSocket(url, protocols);
  };
  WrappedWebSocket.prototype = NativeWebSocket.prototype;
  ["CONNECTING", "OPEN", "CLOSING", "CLOSED"].forEach(function (k) {
    WrappedWebSocket[k] = NativeWebSocket[k];
  });
  window.WebSocket = WrappedWebSocket;

  // --- Grabadora de respuestas -----------------------------------------------------
  //
  // Este script corre **antes** que webmssdk, así que cuando el SDK envuelve
  // `window.fetch` envuelve el nuestro. La cadena queda:
  //
  //     página → wrapper del SDK (firma la URL) → wrapper nuestro → fetch nativo
  //
  // Es decir: nuestra capa ve la URL **ya firmada** y la `Response` real. Eso permite
  // dos cosas que antes no se podían: leer el cuerpo sin repetir la petición, y ver
  // exactamente qué pide el reproductor de verdad cuando arranca solo.
  var WATCH = /\/webcast\//;
  var MAX_CAPTURES = 40;
  window.__ttlCaptures = [];

  var record = function (entry) {
    try {
      window.__ttlCaptures.push(entry);
      while (window.__ttlCaptures.length > MAX_CAPTURES) {
        window.__ttlCaptures.shift();
      }
    } catch (e) {}
  };

  var nativeFetch = window.fetch ? window.fetch.bind(window) : null;
  if (nativeFetch) {
    window.fetch = function (input, init) {
      var url = "";
      try {
        url = typeof input === "string" ? input : (input && input.url) || "";
      } catch (e) {}

      var promise = nativeFetch(input, init);
      if (!WATCH.test(url)) return promise;

      // El `.then` se registra antes de devolver la promesa, así que el clon se hace
      // antes de que la página consuma el cuerpo.
      promise.then(
        function (res) {
          var meta = { url: res.url || url, status: res.status, kind: res.type, via: "fetch" };
          var clone;
          try {
            clone = res.clone();
          } catch (e) {
            record(Object.assign(meta, { error: "no se pudo clonar: " + e }));
            return;
          }
          clone.arrayBuffer().then(
            function (buf) {
              // Solo se guarda el cuerpo de lo que sirve para abrir el WebSocket; del
              // resto basta el tamaño, y guardarlo llenaría la cola de ruido.
              var keep = /\/webcast\/im\/fetch\//.test(meta.url) && buf.byteLength < 400000;
              record(
                Object.assign(meta, {
                  bytes: buf.byteLength,
                  body: keep ? b64(buf) : "",
                  text:
                    !keep && buf.byteLength && buf.byteLength < 400
                      ? String.fromCharCode.apply(null, new Uint8Array(buf))
                      : "",
                })
              );
            },
            function (e) {
              record(Object.assign(meta, { error: String(e) }));
            }
          );
        },
        function (e) {
          record({ url: url, via: "fetch", error: String(e) });
        }
      );
      return promise;
    };
  }

  // Mismo razonamiento para XHR: webmssdk parchea los dos caminos y el reproductor usa
  // uno u otro según la versión del bundle.
  var xhrProto = window.XMLHttpRequest && window.XMLHttpRequest.prototype;
  if (xhrProto && xhrProto.open && xhrProto.send) {
    var nativeOpen = xhrProto.open;
    var nativeSend = xhrProto.send;
    xhrProto.open = function (method, url) {
      try {
        this.__ttlUrl = String(url);
      } catch (e) {}
      return nativeOpen.apply(this, arguments);
    };
    xhrProto.send = function () {
      var self = this;
      try {
        if (self.__ttlUrl && WATCH.test(self.__ttlUrl)) {
          self.addEventListener("loadend", function () {
            var meta = {
              url: self.responseURL || self.__ttlUrl,
              status: self.status,
              via: "xhr",
            };
            try {
              var body = self.response;
              if (body instanceof ArrayBuffer) {
                record(Object.assign(meta, { bytes: body.byteLength, body: b64(body) }));
              } else {
                // Con `responseType` de texto el cuerpo binario ya viene corrompido por
                // la decodificación UTF-8: se anota que pasó, pero no se usa.
                record(
                  Object.assign(meta, {
                    error: "responseType=" + (self.responseType || "text") + ", ilegible como binario",
                  })
                );
              }
            } catch (e) {
              record(Object.assign(meta, { error: String(e) }));
            }
          });
        }
      } catch (e) {}
      return nativeSend.apply(this, arguments);
    };
  }

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
      // Se busca por el path de la URL pedida: el puente firma cualquier endpoint de
      // TikTok LIVE, no solo /webcast/im/fetch/.
      var needle = req.url;
      try { needle = new URL(req.url).pathname; } catch (e) {}

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
          if (entries[j].name.indexOf(needle) !== -1) {
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
          message: "la petición no llegó a salir: sin entrada de " + needle + " en el timeline",
        });
        return;
      }

      post({
        type: "signed",
        request_id: req.request_id,
        url: signed,
        cookie: document.cookie,
        page: location.href,
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

  // Aviso de que este documento ya tiene la sesión puesta. El motor lo usa para saber
  // cuándo puede salir de la página de arranque hacia la de verdad: navegar antes sería
  // pedir la página como anónimo, que es justo lo que se quiere evitar.
  post({
    type: "session",
    installed: sessionInstalled,
    host: location.hostname,
    cookie: document.cookie,
  });

  // Cómo se ve el entorno desde dentro. Los params de la query tienen que decir esto
  // mismo: si el UA dice una cosa y `browser_language` otra, es incoherencia detectable
  // (`docs/06-risks-and-ops.md` §1). Se lee de la página en vez de adivinarlo.
  var entorno = function () {
    var region = "";
    var language = "";
    try {
      var raw = document.getElementById("__UNIVERSAL_DATA_FOR_REHYDRATION__");
      var ctx = JSON.parse(raw.textContent).__DEFAULT_SCOPE__["webapp.app-context"];
      region = ctx.region || "";
      language = ctx.language || "";
    } catch (e) {}
    var browserLanguage = navigator.language || "en-US";
    var tz = "";
    try {
      tz = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
    } catch (e) {}
    return {
      // `navigator.language` is `C` in the current WebKitGTK image, while TikTok's
      // application context and its own webcast query use `en`. Prefer the context;
      // falling back to the navigator is still correct on pages without SSR data.
      language: language || browserLanguage.split("-")[0],
      browser_language: browserLanguage,
      tz_name: tz,
      region: region,
      screen_width: screen.width || 1920,
      screen_height: screen.height || 1080,
    };
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
      post({ type: "ready", sdk_version: version, env: entorno() });
    } else if (Date.now() - t0 > 30000) {
      clearInterval(poll);
      post({ type: "error", request_id: 0, message: "sdk_not_ready" });
    }
  }, 100);
})();
