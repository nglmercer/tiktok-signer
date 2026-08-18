// Synthetic browser shim for webmssdk, with per-path access recording.
// No browser, no network: every global the bundle resolves comes from here.

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
  const elementShim = () => ({
    style: {}, setAttribute() {}, getAttribute: () => null, appendChild: (c) => c,
    removeChild: (c) => c, addEventListener() {}, removeEventListener() {},
    getContext: () => null, remove() {},
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
    getRandomValues: (a) => { for (let i = 0; i < a.length; i++) a[i] = (i * 7 + 13) & 255; return a; },
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
    atob: (s) => Buffer.from(s, 'base64').toString('binary'),
    btoa: (s) => Buffer.from(s, 'binary').toString('base64'),
    Image: function () { return {}; }, WebSocket: function () { return {}; },
    PerformanceObserver: function (cb) { return { observe() {}, disconnect() {}, takeRecords: () => [] }; },
    MutationObserver: function (cb) { return { observe() {}, disconnect() {}, takeRecords: () => [] }; },
    IntersectionObserver: function (cb) { return { observe() {}, disconnect() {} }; },
    Event: function (t) { return { type: t }; },
    CustomEvent: function (t) { return { type: t }; },
    Blob: function () { return {}; },
    FormData: function () { return { append() {} }; },
    Headers: function () { return { append() {}, get: () => null, set() {} }; },
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
