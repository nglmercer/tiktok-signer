// JS↔Rust bridge. It is injected with `with_initialization_script`, so it runs **before**
// webmssdk.js. When the SDK patches `fetch`, this bridge is already installed and never
// observes an intermediate state.
//
// Full contract: docs/04-spec-webview-bridge.md.
(function () {
  if (window.__ttlBridge) return;
  window.__ttlBridge = true;

  const TIKTOK_HOSTNAME = /(^|\.)tiktok\.com$/;
  const SESSION_COOKIE_ATTRIBUTES = "; path=/; domain=.tiktok.com; secure";
  const BASE64_CHUNK_BYTES = 0x8000;
  const WEBSOCKET_STATES = Object.freeze({
    CONNECTING: 0,
    OPEN: 1,
    CLOSING: 2,
    CLOSED: 3,
  });
  const WEBSOCKET_CLOSE_CODES = Object.freeze({
    NORMAL: 1000,
    ABNORMAL: 1006,
  });
  const ASYNC_EVENT_DELAY_MS = 0;
  const MAX_CAPTURES = 40;
  const MAX_CAPTURE_BODY_BYTES = 400000;
  const MAX_CAPTURE_TEXT_BYTES = 400;
  const SIGNATURE_TIMELINE_ATTEMPTS = 30;
  const SIGNATURE_TIMELINE_DELAY_MS = 100;
  const SDK_READY_TIMEOUT_MS = 30000;
  const SDK_READY_POLL_INTERVAL_MS = 100;
  const DEFAULT_SCREEN_WIDTH = 1920;
  const DEFAULT_SCREEN_HEIGHT = 1080;
  const HTTP_OK = 200;
  const JS_EVALUATION_PREFIX = "js:";

  var post = function (o) {
    try {
      window.ipc.postMessage(JSON.stringify(o));
    } catch (e) {
      // Without IPC there is nothing useful to do, and the page must keep running.
    }
  };

  // --- Session ---------------------------------------------------------------------
  //
  // Cookies are installed **here**, not through `WebView::set_cookie`.
  //
  // Measured on 2026-08-10 with WebKitGTK 2.52: `set_cookie` writes to a store that the
  // page does not read. Rust can read those cookies back, but they never appear in
  // `document.cookie` or in requests. TikTok then renders an anonymous session and
  // `/webcast/im/fetch/` returns HTTP 200 with zero bytes.
  //
  // This script runs at the correct origin before page code. `document.cookie` cannot
  // set `HttpOnly`; that does not matter because the server receives the same `Cookie`
  // header either way.
  var sessionInstalled = 0;
  try {
    if (TIKTOK_HOSTNAME.test(location.hostname)) {
      var pairs = window.__ttlSession || [];
      for (var i = 0; i < pairs.length; i++) {
        document.cookie = pairs[i][0] + "=" + pairs[i][1] + SESSION_COOKIE_ATTRIBUTES;
        sessionInstalled++;
      }
    }
  } catch (e) {}

  // Spreading a large Uint8Array overflows the stack, so encode in chunks.
  var b64 = function (buf) {
    var bytes = new Uint8Array(buf);
    var s = "";
    for (var i = 0; i < bytes.length; i += BASE64_CHUNK_BYTES) {
      s += String.fromCharCode.apply(null, bytes.subarray(i, i + BASE64_CHUNK_BYTES));
    }
    return btoa(s);
  };

  // WebSocket URLs opened by the page itself. Wrapping the constructor is safe:
  // webmssdk patches `fetch` and `XMLHttpRequest`, not this. It lets us compare our URI
  // with the real player URI.
  //
  // `window.__ttlBlockWs = true` stops the page from connecting by returning an inert
  // socket without making a network request. Two connections to the same room with the
  // same session interfere: TikTok accepts the second one but sends it no events.
  window.__ttlWsUrls = [];
  // The initialization script may request that player sockets are blocked. Do not reset
  // it here: this file runs after that script and the value must survive until the player
  // tries to connect.
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
    this.readyState = WEBSOCKET_STATES.CONNECTING;
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
    // connection. This object creates no WebKit network loader.
    setTimeout(function () {
      if (self.readyState !== WEBSOCKET_STATES.CONNECTING) return;
      self.readyState = WEBSOCKET_STATES.CLOSED;
      self.__ttlDispatch("error", blockedEvent("error"));
      self.__ttlDispatch(
        "close",
        blockedEvent("close", {
          code: WEBSOCKET_CLOSE_CODES.ABNORMAL,
          reason: "blocked",
          wasClean: false,
        })
      );
    }, ASYNC_EVENT_DELAY_MS);
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
    if (this.readyState !== WEBSOCKET_STATES.OPEN) {
      var error = new Error("WebSocket is not open");
      error.name = "InvalidStateError";
      throw error;
    }
  };
  BlockedWebSocket.prototype.close = function (code, reason) {
    if (
      this.readyState === WEBSOCKET_STATES.CLOSING ||
      this.readyState === WEBSOCKET_STATES.CLOSED
    ) return;
    this.readyState = WEBSOCKET_STATES.CLOSING;
    var self = this;
    setTimeout(function () {
      if (self.readyState === WEBSOCKET_STATES.CLOSED) return;
      self.readyState = WEBSOCKET_STATES.CLOSED;
      self.__ttlDispatch(
        "close",
        blockedEvent("close", {
          code: typeof code === "number" ? code : WEBSOCKET_CLOSE_CODES.NORMAL,
          reason: reason == null ? "" : String(reason),
          wasClean: true,
        })
      );
    }, ASYNC_EVENT_DELAY_MS);
  };
  BlockedWebSocket.CONNECTING = WEBSOCKET_STATES.CONNECTING;
  BlockedWebSocket.OPEN = WEBSOCKET_STATES.OPEN;
  BlockedWebSocket.CLOSING = WEBSOCKET_STATES.CLOSING;
  BlockedWebSocket.CLOSED = WEBSOCKET_STATES.CLOSED;
  var WrappedWebSocket = function (url, protocols) {
    try {
      window.__ttlWsUrls.push(String(url));
    } catch (e) {}
    if (window.__ttlBlockWs) {
      // Intercepting is preferable to throwing: an exception here would break the player
      // where it does not expect failures. The stub also avoids WebKit loader errors from
      // the deliberately invalid socket previously used here.
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

  // --- Response recorder -----------------------------------------------------------
  //
  // This script runs **before** webmssdk, so when the SDK wraps `window.fetch` it wraps
  // ours. The resulting chain is:
  //
  //     page → SDK wrapper (signs the URL) → our wrapper → native fetch
  //
  // Our layer sees the **already signed** URL and the real `Response`. It can read the
  // body without repeating the request and observe exactly what the real player requests.
  var WATCH = /\/webcast\//;
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

      // Register `.then` before returning the promise, so cloning happens before the page
      // consumes the body.
      promise.then(
        function (res) {
          var meta = { url: res.url || url, status: res.status, kind: res.type, via: "fetch" };
          var clone;
          try {
            clone = res.clone();
          } catch (e) {
            record(Object.assign(meta, { error: "could not clone response: " + e }));
            return;
          }
          clone.arrayBuffer().then(
            function (buf) {
              // Keep bodies only for data required to open the WebSocket. For everything
              // else the size is sufficient, and retaining it would fill the queue with noise.
              var keep = /\/webcast\/im\/fetch\//.test(meta.url) &&
                buf.byteLength < MAX_CAPTURE_BODY_BYTES;
              record(
                Object.assign(meta, {
                  bytes: buf.byteLength,
                  body: keep ? b64(buf) : "",
                  text:
                    !keep && buf.byteLength && buf.byteLength < MAX_CAPTURE_TEXT_BYTES
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

  // The same reasoning applies to XHR: webmssdk patches both paths and the player chooses
  // one or the other depending on the bundle version.
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
                // A text `responseType` has already corrupted a binary body through UTF-8
                // decoding. Record that fact, but never use the body.
                record(
                  Object.assign(meta, {
                    error: "responseType=" + (self.responseType || "text") + ", unreadable as binary",
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

  // Sign without reading the response (Plan B in docs/01-architecture.md §D2).
  //
  // `webcast.tiktok.com/webcast/im/fetch/` does **not** return CORS headers, so the page
  // cannot read the body: `fetch` resolves to `undefined` and a pristine iframe `fetch`
  // reports "Load failed".
  //
  // The request still leaves **signed**: webmssdk adds X-Bogus, X-Gnarly, X-Dynosaur, and
  // msToken. The signed URL appears in the Performance Timeline even when the response is
  // unreadable. The bridge returns that URL and Rust repeats it through its HTTP client,
  // where CORS does not apply.
  window.__ttlSign = async function (req) {
    try {
      // Match the requested URL path: the bridge signs every TikTok LIVE endpoint, not
      // only /webcast/im/fetch/.
      var needle = req.url;
      try { needle = new URL(req.url).pathname; } catch (e) {}

      // Inspect only new entries; a previous signature could leave an expired URL here.
      var offset = performance.getEntriesByType("resource").length;

      // `no-cors` avoids a console error; the request still leaves and is still signed.
      try {
        await fetch(req.url, {
          method: "GET",
          credentials: "include",
          mode: "no-cors",
        });
      } catch (e) {
        // A read failure is expected; only the outgoing request matters.
      }

      var signed = null;
      for (var i = 0; i < SIGNATURE_TIMELINE_ATTEMPTS && !signed; i++) {
        var entries = performance.getEntriesByType("resource");
        for (var j = entries.length - 1; j >= offset; j--) {
          if (entries[j].name.indexOf(needle) !== -1) {
            signed = entries[j].name;
            break;
          }
        }
        if (!signed) {
          await sleep(SIGNATURE_TIMELINE_DELAY_MS);
        }
      }

      if (!signed) {
        post({
          type: "error",
          request_id: req.request_id,
          message: "request did not leave: no " + needle + " entry in the performance timeline",
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

  // Flow step 1 (docs/00-research.md §1): it is unsigned, but still runs from the page
  // to reuse the real session and User-Agent.
  //
  // - `req.url`  → text GET (the uniqueId → roomId lookup).
  // - no `url`   → rendered DOM, the only way to see who is live: /live does not include
  //   that data in HTML because the client renders it.
  window.__ttlText = async function (req) {
    try {
      // `js:<expression>` evaluates in the page and returns text. It is a diagnostic
      // bridge path (page requests and available symbols); it never participates in signing.
      if (req.url && req.url.indexOf(JS_EVALUATION_PREFIX) === 0) {
        var value = eval(req.url.slice(JS_EVALUATION_PREFIX.length));
        if (value && typeof value.then === "function") {
          value = await value;
        }
        post({
          type: "text",
          request_id: req.request_id,
          status: HTTP_OK,
          body: typeof value === "string" ? value : JSON.stringify(value),
        });
        return;
      }
      if (!req.url) {
        post({
          type: "text",
          request_id: req.request_id,
          status: HTTP_OK,
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

  // Notify Rust that this document has installed the session. The engine uses it to decide
  // when it can navigate from the bootstrap document to the real page. Navigating first
  // would request the page anonymously.
  post({
    type: "session",
    installed: sessionInstalled,
    host: location.hostname,
    cookie: document.cookie,
  });

  // The environment visible from inside the page. Query parameters must report the same
  // values: a mismatch between the User-Agent and `browser_language` is detectable
  // (`docs/06-risks-and-ops.md` §1). Read it from the page instead of guessing.
  var environment = function () {
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
      screen_width: screen.width || DEFAULT_SCREEN_WIDTH,
      screen_height: screen.height || DEFAULT_SCREEN_HEIGHT,
    };
  };

  // Readiness gate: without `byted_acrawler`, signatures lack X-Gnarly and TikTok rejects
  // them with a symptom that points to the wrong component.
  var t0 = Date.now();
  var poll = setInterval(function () {
    if (typeof window.byted_acrawler !== "undefined") {
      clearInterval(poll);
      var version = null;
      try {
        version = window.byted_acrawler.version || null;
      } catch (e) {}
      post({ type: "ready", sdk_version: version, env: environment() });
    } else if (Date.now() - t0 > SDK_READY_TIMEOUT_MS) {
      clearInterval(poll);
      post({ type: "error", request_id: 0, message: "sdk_not_ready" });
    }
  }, SDK_READY_POLL_INTERVAL_MS);
})();
