# 01 — Architecture

The system is split into nine crates:

- `ttl-sign-core`: pure data types, ordered queries, cookies, presets, room lookup,
  a stable protobuf event subset, and generated schema bindings with bounded dynamic decoding.
- `ttl-sign-headless`: browser-free `SignerBackend`; signs through an external signer process,
  navigation, and page-owned WebSocket relay. It implements the backend contract as the
  optional live oracle.
- `ttl-sign-replay`: versioned fixture loader and deterministic offline backend.
- `ttl-sign-native`: staged headless request/environment/signing/transport pipeline. The
  signing transformation remains an isolated, incomplete research boundary.
- `ttl-sign-lab`: research-only observations, safe value digests, classified differences,
  and oracle/candidate comparison.
- `ttl-live-ws`: signed WebSocket URI construction, handshake, room entry, heartbeats,
  acknowledgements, and frame decoding.
- `ttl-sign-server`: thin backend-agnostic HTTP integration layer.
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

The server depends only on the capability-specific `ttl_sign_core::SignerBackend` contract:

```text
ttl-sign-server -> SignerBackend
                     |-- Headless Signer (live)
                     |-- ReplayBackend (offline integration)
                     |-- NativeBackend (headless target)
                     `-- MockBackend (unit tests)
```

The contract takes a `TransportRequest` and returns the existing three-way `SignOutcome`.
Browser navigation, IPC, room discovery, and gift lookup are deliberately absent. The HTTP
library therefore compiles and its contract tests run without Wry or WebKitGTK. The live
binary enables the `headless` feature explicitly; the offline binary enables `replay`.

No workspace member depends on a browser engine, and CI asserts it. `cargo test` is the headless
suite; live-oracle compilation and execution are separate, explicit checks.

The signer's environment shim is installed before the bundle is evaluated. It wraps the
native WebSocket constructor, leaves TikTok's connection behavior intact, and mirrors
open/frame/close events to Rust. TikTok's page owns signing, room entry, and heartbeats.

The older live signed-fetch replay path remains available for diagnostics and compatibility.
It is separate from `ReplayBackend`, which never uses the network and replays only sanitized
observable fixture data.

`ttl-live-proto` builds the pinned v3 schemas during compilation, emitting both generated
types (under `ttl_live_proto::v3`) and a descriptor registry. `ttl-live-events` uses that
registry for descriptor-driven, bounded decoding of page-owned WebSocket traffic, so a future
field or method remains observable instead of making the listener depend on a stale recursive
generated type. `ttl-sign-core` no longer carries a schema of its own.

One signer owns one cookie session. Sessions must not be shared between rooms because
that increases rate-limit and anti-bot risk.

The signer runs as a subprocess. Tokio awaits it on a worker thread and sends
requests through an event-loop proxy. The WebSocket crate does not reconnect internally:
signed URLs expire and the orchestrator must start a fresh flow.

The server returns typed rejection responses instead of treating an empty HTTP 200 body
as a successful payload. Mock, replay, and native backends run through a shared behavioral
contract suite.

## Research data safety

Signing fixtures live under `fixtures/signing/<case>/case.json` and carry
`fixture_version: 1`. The loader rejects unsupported versions, duplicate requests,
incomplete successful transports, signed query strings, mixed identities, and sensitive
values that are not replaced by `fixture-*` placeholders. Unknown requests fail explicitly.

`ttl-sign-lab` removes URL queries and serializes sensitive values only as SHA-256 digests
plus byte lengths. Its JSON includes backend/runtime/OS/timestamp/sanitization metadata and
classified differences instead of a single match bit.

Experiment plans are schema-versioned and typed. Every non-baseline case must differ from
the baseline in exactly one declared request, signing, or environment field; zero- and
multi-variable changes are rejected before a browser starts. Individual traces use a fresh
fresh guest identity. Query differentials deliberately interleave baseline and experiment
inside one ephemeral identity so browser-state differences do not dominate the comparison.
Captures are create-new and contain a sanitized replay case plus a structured observation.
Sensitive declared signing fields are represented by equality-preserving digests, not copied
verbatim.
