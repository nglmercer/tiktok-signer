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

## Headless migration status

- M0 baseline: event fixtures and explicit signing cases exist; current invariants and the
  WebView dependency boundary are documented.
- M1 backend abstraction: complete. The server library has no direct WebView dependency.
- M2 mock backend: complete. HTTP success, validation, rejection, timeout, unavailable, and
  transport-error mappings run headlessly.
- M3 replay backend: complete for schema version 1 and the initial success/rejection/timeout
  corpus. Corpus breadth still needs to grow from controlled oracle observations.
- M4 property/fuzz foundations: query encoding and URI sanitization properties plus URI,
  protobuf, and fixture fuzz targets exist. Corpus regression and bounds coverage can grow.
- M5 research lab: complete for backend output observation, structured differential JSON,
  individual-case comparison, and an explicit WebView-vs-replay command.
- M6 research corpus: active. A typed one-variable matrix, safe WebView runner,
  non-overwriting URL traces, same-identity paired query experiments, SDK bundle identity,
  and entropy-aware diffs exist. The first sanitized webmssdk/VM profile is committed;
  broader controlled input coverage is still needed.
- M7 native backend: staged skeleton complete. Canonical inputs, fixed time, environment,
  algorithm, and transport are separate; the real signing algorithm is not implemented.
- M8 differential convergence: started. The fetch-signing output structure and two distinct
  SDK entry paths are confirmed. VM instruction/call-graph recovery and native value
  convergence are still pending.
- M9 headless integration: replay and native test backends pass the shared contract. A native
  live-compatible production path is still pending.
- M10 optional WebView: default tests and the replay server are browser-free. The current live
  production server still uses the explicit `webview` feature.
