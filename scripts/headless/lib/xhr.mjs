// A real `XMLHttpRequest` for the sandbox, backed by Node's fetch.
//
// The live player's transport client issues `/webcast/im/fetch/` with `new XMLHttpRequest()`,
// `withCredentials = true`, and a form-urlencoded `Content-Type` — never with `fetch` — and
// webmssdk hooks the two paths separately (`XHRSignTime` and `fetchSignTime` are distinct fields in
// its state). So the XHR route is a different signing route, and it has to be driven through
// something the SDK's hooks on `open`, `setRequestHeader`, and `send` can actually operate on.
//
// A stub that dropped writes would make those hooks no-ops and look exactly like an SDK that adds
// nothing — the same trap the `Headers` stub set earlier. Everything the SDK adds, query parameters
// and headers alike, is captured and handed to `onRecord`.
//
// Shared by `xhr-transport.mjs` and `im-fetch-bisect.mjs`: a divergence between the probe that
// measures a variant and the probe that sends it would make the measurement untransferable.

/// Headers a Chromium XHR sends that Node does not. Listed in docs/12 as a Phase C suspect; they
/// have since been measured to change nothing, so they stay switchable rather than assumed.
export const CHROMIUM_CLIENT_HINTS = {
  accept: '*/*',
  'accept-encoding': 'gzip, deflate, br',
  'sec-ch-ua': '"Chromium";v="131", "Not_A Brand";v="24", "Google Chrome";v="131"',
  'sec-ch-ua-mobile': '?0',
  'sec-ch-ua-platform': '"Linux"',
  'sec-fetch-dest': 'empty',
  'sec-fetch-mode': 'cors',
  'sec-fetch-site': 'same-site',
};

/**
 * Build an `XMLHttpRequest` class for a sandbox.
 *
 * @param {object} options
 * @param {string} options.userAgent      sent as `user-agent`, and normally also `navigator.userAgent`
 * @param {string} options.referer        the room page URL
 * @param {() => string} options.cookieHeader  the current jar, serialized; used when `withCredentials`
 * @param {(response: Response) => void} [options.absorb]  called with every response, for Set-Cookie
 * @param {(record: object) => void} [options.onRecord]    called once per completed or failed send
 * @param {boolean} [options.clientHints]  send the Chromium client hints above
 * @param {object} [options.extraHeaders]  additional request headers
 * @param {(url: string) => string} [options.mutateUrl]  last look at the URL before it is sent
 */
export function createXhrClass({
  userAgent,
  referer,
  cookieHeader,
  absorb = () => {},
  onRecord = () => {},
  clientHints = true,
  extraHeaders = {},
  // Applied inside `send`, which the SDK's hook calls *after* it has rewritten the URL with the
  // signature. A wrapper installed on the instance would run before that rewrite and see an
  // unsigned URL — which is how a "signature removed" experiment silently removes nothing.
  mutateUrl = (url) => url,
}) {
  return class XMLHttpRequest {
    constructor() {
      this.readyState = 0;
      this.status = 0;
      this.statusText = '';
      this.response = null;
      this.responseText = '';
      this.responseURL = '';
      this.responseType = '';
      this.withCredentials = false;
      this.timeout = 0;
      this._headers = {};
      this._listeners = {};
      // The SDK reads and reassigns the whole handler surface while wrapping `send`; a missing slot
      // surfaces as "cannot read properties of undefined", not as a clear error.
      this.onreadystatechange = null;
      this.onload = null;
      this.onloadstart = null;
      this.onloadend = null;
      this.onprogress = null;
      this.onerror = null;
      this.onabort = null;
      this.ontimeout = null;
      this.upload = {
        onloadstart: null, onprogress: null, onload: null, onloadend: null,
        onerror: null, onabort: null, ontimeout: null,
        addEventListener() {}, removeEventListener() {}, dispatchEvent: () => true,
      };
    }

    dispatchEvent() {
      return true;
    }

    overrideMimeType() {}

    open(method, url, async = true) {
      this._method = String(method).toUpperCase();
      this._url = String(url);
      this._async = async;
      this.readyState = 1;
    }

    setRequestHeader(name, value) {
      this._headers[String(name)] = String(value);
    }

    addEventListener(type, handler) {
      (this._listeners[type] ||= []).push(handler);
    }

    removeEventListener() {}

    abort() {}

    getAllResponseHeaders() {
      return this._responseHeaders || '';
    }

    getResponseHeader(name) {
      return this._responseHeaderMap?.get(String(name).toLowerCase()) ?? null;
    }

    send(body) {
      // Everything the SDK added is visible here: the final URL and the final header set.
      const added = this._headers;
      this._url = mutateUrl(this._url);
      (async () => {
        const headers = {
          'user-agent': userAgent,
          origin: 'https://www.tiktok.com',
          referer,
          'accept-language': 'en-US,en;q=0.9',
          ...(clientHints ? CHROMIUM_CLIENT_HINTS : {}),
          ...extraHeaders,
          ...added,
        };
        if (this.withCredentials) {
          const cookie = cookieHeader();
          if (cookie) headers.cookie = cookie;
        }
        let record = {
          url: this._url,
          method: this._method,
          sdk_headers: Object.keys(added),
          with_credentials: this.withCredentials,
        };
        try {
          const response = await fetch(this._url, { method: this._method, headers, body });
          absorb(response);
          const buffer = Buffer.from(await response.arrayBuffer());
          this.status = response.status;
          this.readyState = 4;
          this.response = this.responseType === 'arraybuffer'
            ? buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength)
            : buffer.toString('utf8');
          this._responseHeaderMap = new Map(
            [...response.headers].map(([k, v]) => [k.toLowerCase(), v]),
          );
          this._responseHeaders = [...response.headers].map(([k, v]) => `${k}: ${v}`).join('\r\n');
          record = {
            ...record,
            status: response.status,
            bytes: buffer.length,
            push_server: buffer.toString('latin1').includes('wss://'),
          };
          onRecord(record);
          if (this.onreadystatechange) this.onreadystatechange();
          if (this.onload) this.onload();
          for (const handler of this._listeners.load || []) handler({});
        } catch (error) {
          record = { ...record, error: String(error?.message).slice(0, 80) };
          onRecord(record);
          this.readyState = 4;
          if (this.onerror) this.onerror(error);
          for (const handler of this._listeners.error || []) handler(error);
        }
      })();
    }
  };
}
