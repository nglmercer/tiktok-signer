# Fixtures — capture notes

This is the F0 template. The rest of `fixtures/` is **not versioned** because it
contains real session cookies.

## Capture YYYY-MM-DD

- **Room:** `@user` / `room_id=`
- **webmssdk version:** search for `webmssdk` in the DevTools Sources tab
- **User-Agent:**
- **Files:**
  - `f0/im_fetch.curl` — cURL capture of the `/webcast/im/fetch/` request
  - `f0/im_fetch.pb` — binary response body
  - `f0/ws_url.txt` — WebSocket URL opened afterward

### Observations

- Does the query contain `X-Bogus`, `X-Gnarly), or both?
- Which cookies travel with the request? Which are `HttpOnly`?
- Do `browser_name` and `browser_version` match the `User-Agent`?

## Capture 2026-08-10

`fetch-dump` captures the response through the WebView with an authenticated session:

```sh
# provide ~/.config/ttl-signer/session as a cookie header containing sessionid
TTL_TRANSPORT_OUT=fixtures/f0/im_fetch.pb \
  node scripts/headless/transport.mjs /tmp/webmssdk.js <user>
```

- **webmssdk version:** `window.byted_acrawler.version` is `undefined) in the current
  build; the object exposes `frontierSign`, `registerWsSigner`, `init`, `report),
  `setTTWebid`, `setTTWebidV2`, `setTTWid`, `setUserMode`, `getReferer), and
  `isWebmssdk`.

### Observations

- **Query signatures:** `X-Gnarly` (~332 chars), `X-Dynosaur` (~392 chars),
  `X-Bogus=1`, and `msToken`.
- **Cookies:** WebKit's cookie manager returns the session and anti-bot cookies;
  `document.cookie` sees only a subset.
- **Protobuf fields:** `2 = cursor`, `5 = internal_ext`, `7 = route_params),
  and `10 = push_server). Fields `8) and `9` are absent in the captured response.
- **`route_params`:** contains `wrss` and `imprp`; the client places `cursor` and
  `internal_ext` in the WebSocket query.
- **`push_server`:** `wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/`.
- **WebSocket URI:** signed with `X-Gnarly`; see [05](../docs/05-spec-websocket-client.md).

