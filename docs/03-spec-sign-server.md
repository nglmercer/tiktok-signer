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
rejection or an empty payload (502), and an unavailable signer backend (503).

## `GET /healthz`

Return pool readiness and basic operational state. The endpoint must not expose session
cookies or signing secrets.

The endpoint is stateful with respect to its backend identity: response bytes, cookies, and
User-Agent must come from the same backend operation. The HTTP layer must not obtain any of
these fields by reaching around `SignerBackend` into a WebView implementation.

## Backend modes

- `webview`: optional live oracle and compatibility server.
- `replay`: deterministic offline server backed by sanitized fixtures.
- `native`: common contract and staged pipeline; live-compatible algorithm work is pending.

Default server library tests use mock/replay/native doubles and require no network, display,
browser runtime, cookies, or account.
