# .NET SDK implementation plan

## Architecture

- `Ttl.Live.Native` mirrors the public headers in `crates/ttl-live-native/include/ttl/` with explicit Cdecl P/Invokes, C-compatible layouts, pointer/length UTF-8 helpers, ABI checks, a native resolver, and `SafeHandle` owners. Borrowed event, batch, object, field, and descriptor pointers never escape their owner lifetime.
- `Ttl.Live` provides an async multi-client API. Native callbacks clone only the accepted borrowed event handles before returning; batch and following per-event callbacks are coalesced so accepted batch events are decoded once. A bounded per-client dispatcher has both item and estimated-byte budgets, explicit drop policies, and diagnostics. Application handlers run serially off the native worker.
- The public event model keeps method, metadata, raw bytes, full schema fields by default, wire occurrences, unknown values, and typed normalized events. Full tree capture is configurable, and native JSON modes are serialized lazily from copied method/payload data so no borrowed pointer is retained.
- The Rust library remains responsible for networking, signing, protobuf decoding, schema catalogs, and JSON generation. The managed layer does not reimplement those systems.
- One `Ttl.Live` NuGet package carries native assets at `runtimes/{rid}/native/` for `win-x64`, `linux-x64`, `linux-arm64`, `osx-x64`, and `osx-arm64`, assembled from the existing native artifact workflow.

## Repository findings

- The 98 header exports break down to 2 version, 3 error, 8 client, 64 event/batch/field, and 21 schema functions. Existing headers define ABI v1, Cdecl-compatible C entrypoints, `size_t` lengths, byte-sized C `bool`, callback-scoped borrowed events, and synchronous destruction after callback delivery stops. `check-abi-coverage.py` compares complete managed signatures and callback prototypes; C and managed layout tests assert sizes, offsets, enum widths, pointer widths, and callback pointer sizes.
- The Rust workspace already owns discovery, transport, signing, dynamic protobuf decoding, normalized event models, and descriptor generation. The Java SDK conventions favor async lifecycle APIs, typed event models, off-thread subscriber execution, and deterministic close; the .NET wrapper applies those patterns while using the C ABI instead of JNI or Java-side protocol code.
- `.github/workflows/ttl-live-native.yml` already builds all five requested native targets and uploads libraries. The .NET matrix tests consume the same target builds, while a dependent job packages their runtime assets; `.github/workflows/dotnet.yml` adds managed build and offline diagnostic checks.

## Dependencies and supported targets

- .NET 8 and .NET 10; no managed runtime dependencies in the SDK.
- Tests use xUnit and Microsoft.NET.Test.Sdk. Examples use only the SDK and ASP.NET Core shared framework.
- Native dependency: the matching `ttl-live-native` shared library built by the existing Rust workflow. No TikTok credentials are needed by tests.

## Phases

1. Mirror and inventory all 98 header declarations; check managed declarations against headers and built exports.
2. Implement layouts, UTF-8 marshalling, callbacks, owned handles, event/batch/schema/value APIs, and ABI/version diagnostics.
3. Implement normalized and generic event models, serialized async lifecycle transitions, per-waiter cancellation, shared connection timeout, reconnection, multi-client support, callback dispatch, and deterministic disposal.
4. Add RID-aware NuGet assets and make the existing native matrix test the SDK and aggregate its artifacts into a package.
5. Add offline fixture, lifecycle, marshaling, ABI, schema, bounded-queue, relay, and packaging checks on every native matrix platform.
6. Add usage, relay/VRChat snapshot scope documentation, and runnable console, multi-client, inspection, and ASP.NET Core examples.
7. Run .NET tests/build/pack, native regression tests, formatting and complete ABI signature checks; update the existing feature branch PR without merging.

## Risks and mitigations

- ABI drift or platform-specific layout differences: mechanically compare the full header inventory, require ABI version 1, and exercise actual native calls on all five RIDs.
- Borrowed-pointer lifetime and callback shutdown races: clone accepted event handles while callback pointers are valid, root callback entry points, hold userdata until callbacks are cleared and the native client is destroyed, and keep public handlers off the native worker thread.
- Native reconnect does not itself mean a connection succeeded: complete `ConnectAsync` only on the `CONNECTED` callback; terminal states and explicit disconnect complete pending waiters, caller cancellation affects only that caller, and shared timeout stops the native attempt.
- Native assets can be omitted from local package builds: make packaging fail if any required RID library is absent and validate the packed nupkg contents in CI.
- ARM runner availability or local platform mismatch can limit execution: CI uses the existing native target matrix and records any unavailable runner as a limitation rather than asserting coverage.
- The protobuf schema component has modified AGPL-3.0 terms and additional permissions/restrictions. NuGet metadata now carries a component notice and the full upstream terms; the package must not be described as MIT-only.

## PR #5 correction pass (2026-10-10)

- Implemented the lifecycle, bounded queue, callback ownership, event cloning, and lazy JSON changes above. Individual waiter cancellation no longer disconnects a shared connection attempt. Timeout completion occurs after the native disconnect finishes.
- Replaced the relay's competing-consumer channel with per-subscriber bounded fan-out. `/vrchat/events` returns a fixed-URL bounded JSON snapshot with monotonic dedupe IDs, gift repeat/combo metadata, payload limits, JSON escaping, and host-side IP allowlisting.
- Added deterministic callback-driven lifecycle tests, 10,000-event queue pressure, DropOldest ordering/cleanup, recursive clone checks, and live HTTP tests with two simultaneous SSE subscribers.
- Local validation: .NET 8 and .NET 10 build and 35 tests per target pass; the native Rust regression suite passes; the 98-function ABI checker and C layout assertions pass on Linux x64; formatting is verified locally. A Linux x64 partial package and local NuGet consumer also pass. Windows, macOS, and ARM64 execution is delegated to the existing CI matrix and is not claimed as locally executed.
- Pushed to the existing PR branch. GitHub repository Actions permissions report `enabled: false`; the new commit has no status checks. The pre-existing queued run targets the prior head and has no jobs. Full five-RID packaging, Windows/macOS/ARM64 execution, and packaged consumers on those platforms remain unexecuted until repository Actions are enabled. No package or release was published.

## Acceptance criteria

- Every one of the 98 public declarations has an exact Cdecl managed binding and is checked for declaration/export parity.
- All owned resources have deterministic, idempotent cleanup; callback exceptions are contained; user callbacks cannot block the native worker.
- Normalized chat, gift, like, member, social, and room-user models coexist with full schema, raw, and future-event access.
- `ConnectAsync` observes a real connected transition, and cancellation, reconnect, multiple clients, and reentrant shutdown are covered offline.
- The package contains all five native RID assets and resolves them without manual copying.
- Tests, native regression checks, examples, docs, and the requested PR are completed; no live TikTok test is claimed.

## Local decoding benchmark

.NET 10 Release, Linux x64, 3,000 managed captures of the same owned chat fixture after 200 warmups, comparing PR head 812dd6a with this correction pass: allocations fell from 517,496 to 145,121 bytes/event (72%); median capture latency fell from 6,246.8 to 415.6 microseconds (15x); p95 fell from 7,691.8 to 688.6 microseconds. This microbenchmark excludes native decoding, native allocations, and subsequent requested JSON serialization; it is not an end-to-end transport benchmark. Truncated events cache original native JSON to retain budget-dependent representations.
