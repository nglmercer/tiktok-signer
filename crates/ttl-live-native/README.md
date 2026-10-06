# Native TikTok LIVE C ABI v1

`ttl-live-native` builds `ttl_live` as a static and dynamic library. The public
boundary is the curated `include/ttl/ttl.h`, with no generated protobuf structs,
Tokio setup, external server, browser, or mobile/JNI dependency.

Every event in an accepted batch runs through the bounded descriptor decoder.
Chat, gift, like, member, social, and room-user events additionally retain their
stable normalized model. Known schema-only and future unknown methods reach the
same event callback with metadata, ordered fields, and retained raw bytes.

## Build and link

```sh
cargo build --release -p ttl-live-native
cc -std=c11 -Icrates/ttl-live-native/include \
  crates/ttl-live-native/examples/live.c -Ltarget/release -lttl_live \
  -Wl,-rpath,"$PWD/target/release" -o live
./live @creator /path/to/trusted-bundle.js 'sessionid=...; ttwid=...'
```

The application supplies **trusted explicit JavaScript source**, not a URL. No
bundle is vendored or downloaded by this library. Optional explicit cookies are
copied into the signing profile and transport. With no explicit cookies, the
existing guest bootstrap helper acquires an anonymous identity; TikTok may require an account
session to open its message socket. Discovery resolves the creator's unique ID.
Diagnostics use fixed redacted messages; the library installs no logging sink.
An application's tracing configuration remains responsible for its own logs.

Outputs:

| Target | Static | Dynamic |
|---|---|---|
| Linux x86_64 / arm64 | `libttl_live.a` | `libttl_live.so` |
| macOS x86_64 / arm64 | `libttl_live.a` | `libttl_live.dylib` |
| Windows x86_64 MSVC | `ttl_live.lib` | `ttl_live.dll` + `ttl_live.dll.lib` |

Define `TTL_STATIC` when using the Windows static library. Static Linux linking
also needs the platform system libraries, e.g. `-lpthread -ldl -lm`. Windows
static consumers need the Rust target's system libraries; use the dynamic import
library when the application does not already configure those dependencies.
The native workflow builds, tests C/C++ linkage, and uploads libraries plus
headers on all five target platforms. Only locally executed checks establish
local validation; adding the workflow does not mean those remote jobs have run.

## Event and field ownership

Events, field trees, and batches are immutable. Callback values are borrowed for
that callback only. `ttl_event_clone` returns a deep owned event, released with
`ttl_event_free`. Batch-owned events borrow their batch. JSON is owned and must
be released with `ttl_string_free`. Catalog handles are immutable and last for
the library's lifetime. `NULL` releases are safe; double-free and stale handles
are caller errors. Input buffers must be initialized for their lengths.

Length-delimited string views can contain NULs and need not be terminated.
Method buffers also have a final terminator; use `ttl_event_method_len` for an
unusual transport method containing an embedded NUL. The decode-only entry point
rejects embedded NUL in its explicit method argument. UTF-8 validation occurs
before decoding. Unknown schema names, field names, and enum names return NULL.

Field iteration retains wire order and duplicate occurrences. Packed scalars
expand to ordered repeated occurrences and retain wire type 2. Logical types
are distinct from wire categories. Signed, ZigZag, fixed, floating point, bool,
and enum getters are strict; wrong-type access returns false without modifying
outputs. Unknown varint/fixed data remains readable at its wire width. Map entry
helpers avoid synthetic entry names and preserve duplicate keys; absent default
key/value fields return NULL. Schema JSON supplies scalar map defaults and uses
protobuf last-key-wins semantics. Ordered wire fields retain all map entries.
Oneof metadata is generated; active lookup returns the last received occurrence.
The current pin has no oneofs; synthetic generator tests cover schema evolution.

`ttl_event_info_t` provides method/hash, message ID, history, support, schema,
retained raw size, and optional `common.create_time` in server-defined units.
The hash is stable FNV-1a over UTF-8 method bytes, with possible collisions: the
method string is always canonical. No hand-maintained closed event enum filters
messages. The catalog includes every `Webcast*` descriptor, even duplicate short
names in different packages; lookup follows Rust's preferred package resolver.
The descriptor fingerprint is SHA-256, automatically refreshed on a schema build.

The specification used `ttl_event_schema_name` for both event and catalog
handles. C cannot overload it: events use `ttl_event_schema_name`, and catalog
handles use `ttl_event_schema_full_name`.

## JSON policy

- NORMALIZED: the optional convenience representation; NULL/INVALID_STATE if absent.
- SCHEMA: protobuf JSON field names plus ordered `wireFields` metadata.
- FULL: envelope metadata, normalized model when present, named data, ordered
  wire fields, diagnostics, timestamp, and retained payload size.
- COMPACT: explicit compact convenience output; unknowns report method/byte count.
- RAW: metadata, wire fields, and explicitly base64-encoded retained protobuf.

All 64-bit integers (including identifiers and unknown varints) are decimal
strings. 32-bit integers remain numbers. Normalized count/ID fields are strings;
member action is an int32 number. Bytes use `{"$bytes":"base64"}`. Non-finite
floats use `"NaN"`, `"Infinity"`, or `"-Infinity"`. Named repeated values are
arrays; unexpected duplicate optional fields are arrays as well. Named object
properties are deterministic; `wireFields` preserves original occurrence order.
FULL avoids embedding raw payloads by default. Legacy `LiveEvent`/mobile JSON
serialization is preserved separately and retains its prior number policy.

## Worker lifecycle

`create` prepares a warm signer when explicit cookies are supplied. Otherwise
the worker acquires a guest identity and prepares the signer once, keeping it
warm for subsequent attempts and reconnects. `connect` starts the private worker
and current-thread runtime; room discovery and all callbacks are asynchronous.
Callbacks for one client are serialized. Batch callback runs first, then event
callbacks in server order. Multiple clients own independent connection state.
The shared `ttl-live-core` loop also backs the existing mobile frontend.

Callback registration and clearing are synchronized. Clearing from another
thread waits for in-flight delivery so old userdata can be released afterward.
Disconnect disables delivery, cancels pending discovery/signing/open/drain and
backoff waits, and joins the worker. No callback follows completed destruction.
Reentrant disconnect inside a callback requests stop without joining itself;
another thread must join/destroy. Destroy from a callback is refused and sets
INVALID_STATE, leaving the handle alive. Destroy must not race any other API
using that client. C++ exceptions and longjmp across callbacks are forbidden.
A stopped client can be connected again and retains its callback registrations.

Options start with `struct_size`; the original prefix is accepted without the
appended cookie fields. Failures return explicit result codes plus a per-thread
last error, or structured asynchronous error/state callbacks. Retryability is
reported; a retry-after of -1 or HTTP status 0 means unavailable. Transport
errors never include signed URLs, cookies, bundle details, or signer diagnostics.
Malformed individual events are delivered with decode-warning flags and do not
terminate a batch or connection. Invalid outer batches produce a nonfatal
batch-decode error so later batches can continue.

## Defensive limits and validation

Events retain at most a **4 MiB prefix**; oversized events are marked truncated
and skip normalized decoding. Schema decoding bounds depth at 8 and fields at
4096 globally per event, including packed values, with an aggregate 65536-field
budget per batch. Events after field-budget exhaustion are still delivered with
raw bytes and truncation metadata. Truncated events skip generated normalization
to avoid materializing an unbounded repeated-message tree. Fields that exceed a nested
limit retain bytes/truncation metadata; complete retained payload bytes remain
accessible separately. Malformed nested fields fall back to bytes with warnings.
Batches larger than **16 MiB or 8192 events** are rejected before expensive
materialization. WebSocket frame/message and decompression limits are 16 MiB;
an oversized transport frame is a transport failure. These documented bounds
are the exceptions to delivery; lack of a normalizer never drops an event.

```sh
cargo test -p ttl-live-native -p ttl-live-events -p ttl-live-proto -p ttl-live-core
cargo test -p ttl-sign-mobile --features jni,live
cargo clippy -p ttl-live-native -p ttl-live-events -p ttl-live-proto \
  -p ttl-live-core --all-targets -- -D warnings
cc -std=c11 -Wall -Wextra -Werror -Icrates/ttl-live-native/include \
  crates/ttl-live-native/tests/smoke.c -Ltarget/release -lttl_live \
  -Wl,-rpath,"$PWD/target/release" -o /tmp/ttl-native-smoke
/tmp/ttl-native-smoke
python3 scripts/native/check-exports.py target/release/libttl_live.so
cargo +nightly fuzz run event_envelope -- -max_total_time=30
cargo +nightly fuzz run native_decode -- -max_total_time=30
```

Tests include all catalog descriptors, logical primitive and packed types,
map/oneof/enum generator metadata, captured known schema-only events, future
methods/fields, malformed successors, embedded NUL text, deep clone/free,
callback clearing, reentrant stop, repeated destruction, independent clients,
actual loopback WebSocket delivery, and cancellation during signing/backoff.
Captured fixtures are the existing repository captures; synthetic descriptor,
wire, mock signing bundle and loopback fixtures are explicitly test-only.
Rust/Node normalized goldens remain unchanged; native JSON shares Rust's new
serializer. Existing real-bundle signature tests skip when `TTL_BUNDLE` is
unavailable; loopback/mock tests do not establish live TikTok acceptance.
No OBS, GUI, overlay, Node/Python bindings, or other consumers are included.

## Schema licensing notices

The curated ABI and new Rust code use the workspace license. Linked binaries
also contain the pinned schemas governed by upstream's modified AGPL-3.0 with
additional permissions. Artifact packages retain the workspace license plus
`protobuf-license/LICENSE.upstream`, upstream provenance, and the existing
protobuf crate's licensing explanation. Refer to those original terms when
redistributing; the complete binary is not labelled solely as MIT.
