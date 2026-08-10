# 04 — WebView bridge specification

The initialization script runs before TikTok's page scripts and before
`webmssdk.js` patches `fetch`.

Rust sends JSON through `evaluate_script("__ttlSign(<json>)")`. JavaScript sends JSON
back through `window.ipc.postMessage`. Every asynchronous request has a `request_id`.

The bridge reports:

- `ready`: the SDK is available;
- `signed`: a signed URL and the cookies used for the request;
- `text`: rendered DOM or diagnostic JavaScript output;
- `ws_open`, `ws_frame`, `ws_close`, and `ws_error`: events from TikTok's own socket;
- `error`: a page-side failure.

The primary flow preserves the page-owned socket and relays binary frames as base64 IPC
messages. Blob conversions are serialized to preserve frame order. The bridge never sends,
closes, or replaces that native socket.

The diagnostic signing flow can still observe a signed URL through the Performance
Timeline and replay it from Rust. Cookies are merged from `document.cookie` and WebKit's
cookie manager, with manager values taking precedence because it can read `HttpOnly` cookies.

The bridge blocks player WebSockets when requested, preventing a second connection to
the same room from consuming or interfering with the signing session.
