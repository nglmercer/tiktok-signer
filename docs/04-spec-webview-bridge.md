# 04 — WebView bridge specification

The initialization script runs before TikTok's page scripts and before
`webmssdk.js` patches `fetch`.

Rust sends JSON through `evaluate_script("__ttlSign(<json>)")`. JavaScript sends JSON
back through `window.ipc.postMessage`. Every asynchronous request has a `request_id).

The bridge reports:

- `ready`: the SDK is available;
- `signed`: a signed URL and the cookies used for the request;
- `text`: rendered DOM or diagnostic JavaScript output;
- `error`: a page-side failure.

The current flow observes the signed URL through the Performance Timeline and replays
the request from Rust. It does not attempt to read the cross-origin response body from
the page. Cookies are merged from `document.cookie` and WebKit's cookie manager,
with manager values taking precedence because it can read `HttpOnly) cookies.

The bridge blocks player WebSockets when requested, preventing a second connection to
the same room from consuming or interfering with the signing session.
