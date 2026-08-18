// Synthetic browser shim for webmssdk, with per-path access recording.
// No browser, no network: every global the bundle resolves comes from here.

import { CANVAS_DATA_URL, CANVAS_HEIGHT, CANVAS_WIDTH, inkAt, measureTextWidth }
  from './lib/canvas.mjs';

// --- what this shim needs from its host -----------------------------------------------------------
//
// Everything below runs unchanged in Node, Deno, a browser, and an embedded engine. That is
// deliberate: the signer is meant to move into a Rust process with QuickJS or Boa inside it, and a
// sandbox that reaches for `node:zlib` cannot go there. So the two things a bare engine does not
// have are named here and nowhere else:
//
//   - randomness, injected as `TTL_RANDOM_SOURCE` or taken from the engine's own `crypto`
//   - base64, implemented below in plain JavaScript rather than through `Buffer`
//
// The canvas fingerprint used to need a third — DEFLATE, to build its PNG — and is now a generated
// constant in `lib/canvas.mjs`. The engine contract that remains is in `scripts/headless/README.md`.

const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

/// `btoa`: binary string in, base64 out. No `Buffer`, so it runs anywhere.
function encodeBase64(input) {
  const text = String(input);
  let out = '';
  for (let at = 0; at < text.length; at += 3) {
    const a = text.charCodeAt(at) & 255;
    const hasB = at + 1 < text.length;
    const hasC = at + 2 < text.length;
    const b = hasB ? text.charCodeAt(at + 1) & 255 : 0;
    const c = hasC ? text.charCodeAt(at + 2) & 255 : 0;
    out += BASE64_ALPHABET[a >> 2];
    out += BASE64_ALPHABET[((a & 3) << 4) | (b >> 4)];
    out += hasB ? BASE64_ALPHABET[((b & 15) << 2) | (c >> 6)] : '=';
    out += hasC ? BASE64_ALPHABET[c & 63] : '=';
  }
  return out;
}

/// `atob`: base64 in, binary string out.
function decodeBase64(input) {
  const text = String(input).replace(/[^A-Za-z0-9+/]/g, '');
  let out = '';
  for (let at = 0; at < text.length; at += 4) {
    const chunk = [0, 1, 2, 3].map((offset) => BASE64_ALPHABET.indexOf(text[at + offset] ?? 'A'));
    const bits = (chunk[0] << 18) | (chunk[1] << 12) | (chunk[2] << 6) | chunk[3];
    out += String.fromCharCode((bits >> 16) & 255);
    if (text[at + 2] !== undefined) out += String.fromCharCode((bits >> 8) & 255);
    if (text[at + 3] !== undefined) out += String.fromCharCode(bits & 255);
  }
  return out;
}

/// A switch the caller sets, from `globalThis.TTL_SHIM_OPTIONS` or from the environment when the
/// host has one. Read at access time rather than at construction, so a probe can flip it per run.
function flag(name) {
  const options = globalThis.TTL_SHIM_OPTIONS;
  if (options && name in options) return Boolean(options[name]);
  return Boolean(globalThis.process?.env?.[name]);
}

/// Where randomness comes from, in order of preference: a host-injected source, then the engine's
/// own `crypto`. An engine with neither cannot sign — the SDK's entropy is not decoration, and a
/// counter dressed up as randomness would produce signatures outside the distribution a browser
/// makes, which is measurable (`X-Dynosaur` came out short when that was tried).
function hostRandomValues(array) {
  const injected = globalThis.TTL_RANDOM_SOURCE;
  if (typeof injected === 'function') return injected(array);
  if (globalThis.crypto?.getRandomValues) return globalThis.crypto.getRandomValues(array);
  throw new Error('no random source: set globalThis.TTL_RANDOM_SOURCE or provide crypto');
}

export function createSandbox() {
const accesses = new Map();
  const record = (path, op, value) => {
    const key = `${path}|${op}`;
    const seen = accesses.get(key) || { path, op, gets: 0, type: typeof value, missing: value === undefined };
    seen.gets++;
    accesses.set(key, seen);
  };
  
  // A recording view over a plain object. Unknown properties return undefined and are recorded as
  // missing, which is exactly the list a shim has to grow to satisfy.
  const view = (name, target) => new Proxy(target, {
    has(t, k) { record(`${name}.${String(k)}`, 'has'); return true; },
    get(t, k) {
      if (k === Symbol.unscopables) return undefined;
      if (k === Symbol.toPrimitive || k === 'toString') return t[k];
      const key = String(k);
      const value = Reflect.get(t, k);
      record(`${name}.${key}`, 'get', value);
      return value;
    },
    set(t, k, v) { record(`${name}.${String(k)}`, 'set', v); t[k] = v; return true; },
  });
  
  const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';
  
  const navigatorShim = view('navigator', {
    userAgent: UA, appVersion: UA.slice(8), platform: 'Linux x86_64', language: 'en-US',
    languages: ['en-US', 'en'], vendor: 'Google Inc.', hardwareConcurrency: 8,
    deviceMemory: 8, maxTouchPoints: 0, webdriver: false, cookieEnabled: true,
    doNotTrack: null, product: 'Gecko', productSub: '20030107', appName: 'Netscape',
    plugins: { length: 0 }, mimeTypes: { length: 0 }, onLine: true,
    connection: { effectiveType: '4g', rtt: 50, downlink: 10, saveData: false },
    sendBeacon: () => true, javaEnabled: () => false,
  });
  const screenShim = view('screen', {
    width: 1920, height: 1080, availWidth: 1920, availHeight: 1050,
    colorDepth: 24, pixelDepth: 24, availLeft: 0, availTop: 0,
  });
  const locationShim = view('location', {
    href: 'https://www.tiktok.com/', protocol: 'https:', host: 'www.tiktok.com',
    hostname: 'www.tiktok.com', origin: 'https://www.tiktok.com', pathname: '/',
    search: '', hash: '', port: '',
  });
  const storageShim = () => view('localStorage', {
    getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {},
    key: () => null, length: 0,
  });
  // A canvas that can return a WebGL context. The SDK keeps a `WEBGL` field in its state, and a
  // `getContext` that always returns null makes that collection silently empty — which shortens
  // the fingerprint rather than failing. TTL_NO_WEBGL restores the empty behaviour.
  const WEBGL_PARAMS = {
    7936: 'WebKit', 7937: 'WebKit WebGL', 7938: 'WebGL 1.0 (OpenGL ES 2.0 Chromium)',
    35724: 'WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)',
    37445: 'Google Inc. (Intel)',
    37446: 'ANGLE (Intel, Mesa Intel(R) UHD Graphics (CML GT2), OpenGL 4.6)',
    3379: 16384, 34076: 16384, 34921: 16, 35660: 16, 35661: 32, 36349: 1024, 34930: 16,
  };
  const webglContext = () => ({
    getParameter: (p) => (p in WEBGL_PARAMS ? WEBGL_PARAMS[p] : 0),
    getExtension: (name) => (name === 'WEBGL_debug_renderer_info'
      ? { UNMASKED_VENDOR_WEBGL: 37445, UNMASKED_RENDERER_WEBGL: 37446 } : null),
    getSupportedExtensions: () => ['ANGLE_instanced_arrays', 'EXT_blend_minmax', 'OES_texture_float'],
    getShaderPrecisionFormat: () => ({ rangeMin: 127, rangeMax: 127, precision: 23 }),
    readPixels() {}, texImage2D() {}, createTexture: () => ({}), bindTexture() {},
    getUniformLocation: () => ({}), uniform1f() {}, uniform2f() {}, deleteBuffer() {},
    createBuffer: () => ({}), bindBuffer() {}, bufferData() {}, createProgram: () => ({}),
    createShader: () => ({}), shaderSource() {}, compileShader() {}, attachShader() {},
    linkProgram() {}, useProgram() {}, getAttribLocation: () => 0, enableVertexAttribArray() {},
    vertexAttribPointer() {}, drawArrays() {}, viewport() {}, clearColor() {}, clear() {},
    canvas: { width: CANVAS_WIDTH, height: CANVAS_HEIGHT, toDataURL: () => CANVAS_DATA_URL },
  });
  const elementShim = () => ({
    style: {}, setAttribute() {}, getAttribute: () => null, appendChild: (c) => c,
    removeChild: (c) => c, addEventListener() {}, removeEventListener() {},
    width: CANVAS_WIDTH, height: CANVAS_HEIGHT,
    toDataURL: () => CANVAS_DATA_URL,
    getContext: (kind) => {
      if (flag('TTL_NO_WEBGL')) return null;
      if (kind === 'webgl' || kind === 'experimental-webgl' || kind === 'webgl2') {
        return webglContext();
      }
      if (kind === '2d') {
        return {
          fillText() {}, strokeText() {}, fillRect() {}, arc() {}, beginPath() {}, closePath() {},
          fill() {}, stroke() {},
          measureText: (text) => ({ width: measureTextWidth(text) }),
          // Real pixels, matching the image `toDataURL` returns. An empty array is a canvas that
          // drew nothing, which is a fingerprint no browser produces.
          getImageData: (x = 0, y = 0, w = CANVAS_WIDTH, h = CANVAS_HEIGHT) => {
            const data = new Uint8ClampedArray(w * h * 4);
            for (let row = 0; row < h; row += 1) {
              for (let column = 0; column < w; column += 1) {
                const at = (row * w + column) * 4;
                const ink = inkAt(x + column, y + row);
                data[at] = ink ? 34 : 250;
                data[at + 1] = ink ? 102 : 250;
                data[at + 2] = ink ? 170 : 250;
                data[at + 3] = 255;
              }
            }
            return { data, width: w, height: h };
          },
          canvas: { width: CANVAS_WIDTH, height: CANVAS_HEIGHT, toDataURL: () => CANVAS_DATA_URL },
        };
      }
      return null;
    },
    remove() {},
  });
  const documentShim = view('document', {
    cookie: '', createElement: () => elementShim(), createTextNode: () => ({}),
    getElementsByTagName: () => [], getElementById: () => null,
    querySelector: () => null, querySelectorAll: () => [],
    addEventListener() {}, removeEventListener() {},
    documentElement: elementShim(), head: elementShim(), body: elementShim(),
    referrer: '', title: '', readyState: 'complete', visibilityState: 'visible',
    location: locationShim, characterSet: 'UTF-8', hidden: false,
  });
  const cryptoShim = view('crypto', {
    // Real entropy by default. A fixed sequence makes runs comparable but pushes the signatures
  // out of the distribution a browser produces — `X-Dynosaur` came out 384 bytes against the
  // 388/392/444 the oracle recorded, and `X-Gnarly` signs over it, so both were short.
  // TTL_DETERMINISTIC restores the fixed sequence for differential work.
  getRandomValues: (a) => {
    if (flag('TTL_DETERMINISTIC')) {
      for (let i = 0; i < a.length; i++) a[i] = (i * 7 + 13) & 255;
      return a;
    }
    return hostRandomValues(a);
  },
    randomUUID: () => '00000000-0000-4000-8000-000000000000',
    subtle: {},
  });
  const performanceShim = view('performance', {
    now: () => 1234.5, timeOrigin: 1700000000000,
    timing: { navigationStart: 1700000000000 }, getEntriesByType: () => [],
  });
  
  const windowTarget = {
    navigator: navigatorShim, screen: screenShim, location: locationShim,
    document: documentShim, crypto: cryptoShim, performance: performanceShim,
    localStorage: storageShim(), sessionStorage: storageShim(),
    innerWidth: 1920, innerHeight: 1080, outerWidth: 1920, outerHeight: 1080,
    devicePixelRatio: 1, screenX: 0, screenY: 0, scrollX: 0, scrollY: 0,
    origin: 'https://www.tiktok.com', isSecureContext: true, name: '',
    addEventListener() {}, removeEventListener() {}, dispatchEvent: () => true,
    setTimeout, clearTimeout, setInterval, clearInterval,
    requestAnimationFrame: (cb) => setTimeout(cb, 0), cancelAnimationFrame: clearTimeout,
    fetch: async () => ({ ok: true, status: 200, text: async () => '', json: async () => ({}) }),
    XMLHttpRequest: function () { return { open() {}, send() {}, setRequestHeader() {}, addEventListener() {} }; },
    atob: decodeBase64,
    btoa: encodeBase64,
    Image: function () { return {}; }, WebSocket: function () { return {}; },
    PerformanceObserver: function (cb) { return { observe() {}, disconnect() {}, takeRecords: () => [] }; },
    MutationObserver: function (cb) { return { observe() {}, disconnect() {}, takeRecords: () => [] }; },
    IntersectionObserver: function (cb) { return { observe() {}, disconnect() {} }; },
    Event: function (t) { return { type: t }; },
    CustomEvent: function (t) { return { type: t }; },
    Blob: function () { return {}; },
    FormData: function () { return { append() {} }; },
    // A working Headers implementation. A stub that discarded writes would silently drop any
    // header the SDK adds to a signed request, which is indistinguishable from the SDK adding
    // none — and header names are part of what a shim has to reproduce.
    Headers: function (init) {
      const map = new Map();
      const norm = (k) => String(k).toLowerCase();
      const self = {
        append(k, v) { const n = norm(k); map.set(n, map.has(n) ? `${map.get(n)}, ${v}` : String(v)); },
        set(k, v) { map.set(norm(k), String(v)); },
        get(k) { const n = norm(k); return map.has(n) ? map.get(n) : null; },
        has(k) { return map.has(norm(k)); },
        delete(k) { map.delete(norm(k)); },
        forEach(fn) { map.forEach((v, k) => fn(v, k, self)); },
        keys: () => map.keys(), values: () => map.values(), entries: () => map.entries(),
        [Symbol.iterator]: () => map.entries(),
      };
      if (init) for (const [k, v] of (typeof init.forEach === 'function' && !Array.isArray(init)
        ? [...init] : Object.entries(init))) self.set(k, v);
      return self;
    },
    Response: function (b, i) { return { ok: true, status: (i && i.status) || 200,
      text: async () => String(b || ''), json: async () => ({}) }; },
    Request: function (u) { return { url: String(u) }; },
    AbortController: function () { return { abort() {}, signal: {} }; },
    queueMicrotask,
    Intl, Math, JSON, Date, Object, Array, Function, String, Number, Boolean, Symbol,
    RegExp, Error, TypeError, RangeError, SyntaxError, Promise, Proxy, Reflect,
    Map, Set, WeakMap, WeakSet, ArrayBuffer, Uint8Array, Uint16Array, Uint32Array,
    Int8Array, Int32Array, Float32Array, Float64Array, DataView,
    TextEncoder, TextDecoder, URL, URLSearchParams, console,
    parseInt, parseFloat, isNaN, isFinite,
    encodeURIComponent, decodeURIComponent, encodeURI, decodeURI, escape, unescape,
    NaN, Infinity, undefined: void 0,
  };
  const windowShim = view('window', windowTarget);
  windowTarget.window = windowShim;
  windowTarget.self = windowShim;
  windowTarget.globalThis = windowShim;
  windowTarget.top = windowShim;
  windowTarget.parent = windowShim;
  
  const sandbox = new Proxy(windowTarget, {
    has: () => true,
    get(t, k) {
      if (k === Symbol.unscopables) return undefined;
      const key = String(k);
      const has = Object.prototype.hasOwnProperty.call(t, key);
      const value = has ? t[key] : undefined;
      record(`global.${key}`, 'get', value);
      return value;
    },
    set(t, k, v) { record(`global.${String(k)}`, 'set', v); t[k] = v; return true; },
  });
  
  
  return { sandbox, windowTarget, accesses,
    view, record,
    load(source) {
      try { new Function('__sandbox', `with(__sandbox){ ${source} \n}`)(sandbox); return null; }
      catch (error) { return { name: error?.name, message: String(error?.message).slice(0, 300) }; }
    },
    surface() {
      return [...accesses.values()]
        .sort((a, b) => a.path.localeCompare(b.path) || a.op.localeCompare(b.op));
    },
  };
}
