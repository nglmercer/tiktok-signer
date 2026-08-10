# 02 — Roadmap

## F0 — Observe the page transport

Record native WebSocket lifecycle events without changing the page socket. Never log or
commit signed query strings or real session cookies.

## F1 — Relay frames

Mirror Blob, ArrayBuffer, typed-array, and text frames through IPC while preserving order.
Decode relayed `WebcastPushFrame` envelopes in Rust.

## F2 — Session and navigation

Create the hidden WebView, install the initialization bridge before the SDK, preserve
session cookies, navigate to the real channel page, and expose a typed event stream.

## F3 — HTTP server

Expose `GET /webcast/fetch` and `GET /healthz`. Return protobuf bytes and the cookie
and User-Agent headers required by compatible clients. Map missing rooms, rejection,
rate limiting, and pool readiness to distinct HTTP statuses.

## F4 — Live validation

Verify that the page opens the webcast socket and that Rust receives room-entry,
heartbeat, and multiple `msg` frames without an external sign server.

## F5 — Operations

Add WebView recycling, a rate limiter, rejection and latency metrics, health reporting,
and long-running authenticated-session validation. Do not add automatic reconnects with
expired signed parameters.
