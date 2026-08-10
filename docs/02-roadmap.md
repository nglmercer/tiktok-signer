# 02 — Roadmap

## F0 — Capture

Capture a real authenticated `/webcast/im/fetch/` response, its cookies, protobuf body,
and WebSocket URL. The capture must not commit real session cookies.

## F1 — Validate without the WebView

Replay the captured request, decode protobuf, construct the WebSocket URI, send room
entry, and receive frames. This separates connection-model failures from signing failures.

## F2 — WebView signer

Create the hidden WebView, install the initialization bridge before the SDK, preserve
session cookies, navigate to the real channel page, and expose a typed signing API.

## F3 — HTTP server

Expose `GET /webcast/fetch` and `GET /healthz`. Return protobuf bytes and the cookie
and User-Agent headers required by compatible clients. Map missing rooms, rejection,
rate limiting, and pool readiness to distinct HTTP statuses.

## F4 — Live WebSocket

Use the shared URI builder, send room entry immediately after the 101 handshake, send
application heartbeats, acknowledge frames, and expose closure to the orchestrator.

## F5 — Operations

Add WebView recycling, a rate limiter, rejection and latency metrics, health reporting,
and long-running authenticated-session validation. Do not add automatic reconnects with
expired signed parameters.

