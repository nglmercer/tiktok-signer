# 01 — Architecture

The system is split into six crates:

- `ttl-sign-core`: pure data types, ordered queries, cookies, presets, room lookup,
  a stable protobuf event subset, and generated schema bindings with bounded dynamic decoding.
- `ttl-sign-webview`: Wry/WebKitGTK event loop, initialization bridge, session cookies,
  navigation, and page-owned WebSocket relay.
- `ttl-live-ws`: signed WebSocket URI construction, handshake, room entry, heartbeats,
  acknowledgements, and frame decoding.
- `ttl-sign-server`: thin HTTP integration layer exposing the signer.
- `ttl-live-proto`: Prost bindings generated from the vendored TikTok Webcast **v3**
  schemas. Schema only — no transport, no normalisation.
- `ttl-live-events`: normalises decoded messages into a stable, version-independent
  event API.

## Transport, schema, normalisation

These are three separate concerns and are kept in three separate crates:

```
TikTok → WebSocket frame → ttl-live-ws → WebcastPushFrame → gzip
       → LiveMessage.payload → ttl-live-proto → ProtoMessageFetchResult
       → BaseProtoMessage.method → ttl-live-events → Chat / Gift / Like / …
```

`ttl-live-ws` keeps its own hand-written decoder for the transport envelope
(`FetchResult`, `PushFrame` in `ttl-sign-core::proto`). That decoder reads a
handful of field numbers, ignores everything else, and has been stable across
TikTok's additive changes; replacing it with the full generated schema would
couple the transport to a large event tree for no benefit. The transport
therefore still hands the consumer a `LiveMessage { log_id, payload }` and the
event layer starts from those bytes.

The two decoders are checked against each other on the same fixtures in
`crates/ttl-live-events/tests/parity.rs`, which is the evidence needed before the
legacy event decoder is retired.

`ttl-live-events` never exposes generated Prost types. Normalisers map the v3
messages onto owned structs, so a future v4 schema means new normalisers rather
than a breaking change for consumers. Anything without a normaliser surfaces as
`LiveEvent::Unknown { method, payload }` with its bytes intact — unrecognised
events are never discarded.

The vendored schemas are pinned to an upstream commit and are never fetched
during `cargo build`; see `crates/ttl-live-proto/README.md`, which also covers
their licence, which is **not** MIT.

## Design decisions

The WebView initialization script is installed before TikTok page scripts. It wraps the
native WebSocket constructor, leaves TikTok's connection behavior intact, and mirrors
open/frame/close events to Rust. TikTok's page owns signing, room entry, and heartbeats.

The older signed-fetch replay path remains available for diagnostics and compatibility,
but the primary transport does not depend on it.

`ttl-live-proto` builds the pinned v3 schemas during compilation, emitting both generated
types (under `ttl_live_proto::v3`) and a descriptor registry. `ttl-live-events` uses that
registry for descriptor-driven, bounded decoding of page-owned WebSocket traffic, so a future
field or method remains observable instead of making the listener depend on a stale recursive
generated type. `ttl-sign-core` no longer carries a schema of its own.

One WebView owns one cookie session. Sessions must not be shared between rooms because
that increases rate-limit and anti-bot risk.

The main WebView event loop owns the GUI thread. Tokio runs on a worker thread and sends
requests through an event-loop proxy. The WebSocket crate does not reconnect internally:
signed URLs expire and the orchestrator must start a fresh flow.

The server returns typed rejection responses instead of treating an empty HTTP 200 body
as a successful payload.
