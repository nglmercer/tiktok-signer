# 03 — Sign-server specification

The server exposes a compatible signing endpoint for TikTok LIVE clients.

## `GET /webcast/fetch`

Required query input is `room_id`. Optional browser parameters may be accepted for
compatibility, but the server generates a coherent `Preset` and returns the actual
User-Agent used for signing.

A successful response contains raw TikTok protobuf bytes and the headers
`X-Set-TT-Cookie` and `X-Set-TT-User-Agent`. The response must never be HTTP 200 with
an empty body.

Use distinct errors for invalid room IDs (400), local rate limiting (429), TikTok
rejection or an empty payload (502), and an unavailable WebView pool (503).

## `GET /healthz`

Return pool readiness and basic operational state. The endpoint must not expose session
cookies or signing secrets.

The endpoint is stateful with respect to the WebView pool: response bytes and cookies
must come from the same WebView instance.

