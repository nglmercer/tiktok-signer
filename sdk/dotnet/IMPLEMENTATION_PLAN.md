# .NET SDK implementation plan

## Architecture

- `Ttl.Live.Native` mirrors the public headers in `crates/ttl-live-native/include/ttl/` with explicit Cdecl P/Invokes, C-compatible layouts, pointer/length UTF-8 helpers, ABI checks, a native resolver, and `SafeHandle` owners. Borrowed event, batch, object, field, and descriptor pointers never escape their owner lifetime.
- `Ttl.Live` provides an async multi-client API. Native callbacks copy event and batch data before returning and only enqueue managed notifications; application handlers run on a managed dispatcher. The public event model keeps the original method, metadata, raw bytes, schema fields, wire occurrences, and unknown values alongside typed normalized events.
- The Rust library remains responsible for networking, signing, protobuf decoding, schema catalogs, and JSON generation. The managed layer does not reimplement those systems.
- One `Ttl.Live` NuGet package carries native assets at `runtimes/{rid}/native/` for `win-x64`, `linux-x64`, `linux-arm64`, `osx-x64`, and `osx-arm64`, assembled from the existing native artifact workflow.

## Repository findings

- The 98 header exports break down to 2 version, 3 error, 8 client, 64 event/batch/field, and 21 schema functions. Existing headers define ABI v1, Cdecl-compatible C entrypoints, `size_t` lengths, byte-sized C `bool`, callback-scoped borrowed events, and synchronous destruction after callback delivery stops.
- The Rust workspace already owns discovery, transport, signing, dynamic protobuf decoding, normalized event models, and descriptor generation. The Java SDK conventions favor async lifecycle APIs, typed event models, off-thread subscriber execution, and deterministic close; the .NET wrapper applies those patterns while using the C ABI instead of JNI or Java-side protocol code.
- `.github/workflows/ttl-live-native.yml` already builds all five requested native targets and uploads libraries. The .NET matrix tests consume the same target builds, while a dependent job packages their runtime assets; `.github/workflows/dotnet.yml` adds managed build and offline diagnostic checks.

## Dependencies and supported targets

- .NET 8 and .NET 10; no managed runtime dependencies in the SDK.
- Tests use xUnit and Microsoft.NET.Test.Sdk. Examples use only the SDK and ASP.NET Core shared framework.
- Native dependency: the matching `ttl-live-native` shared library built by the existing Rust workflow. No TikTok credentials are needed by tests.

## Phases

1. Mirror and inventory all 98 header declarations; check managed declarations against headers and built exports.
2. Implement layouts, UTF-8 marshalling, callbacks, owned handles, event/batch/schema/value APIs, and ABI/version diagnostics.
3. Implement normalized and generic event models, async connection/state transitions, cancellation, reconnection options, multi-client support, callback dispatch, and deterministic disposal.
4. Add RID-aware NuGet assets and make the existing native matrix test the SDK and aggregate its artifacts into a package.
5. Add offline fixture, lifecycle, marshaling, ABI, schema, and packaging checks on every native matrix platform.
6. Add usage, relay/VRChat scope documentation, and runnable console, multi-client, inspection, and ASP.NET Core examples.
7. Run .NET tests/build/pack, native regression tests, formatting and ABI parity checks; review and publish the feature branch PR without merging.

## Risks and mitigations

- ABI drift or platform-specific layout differences: mechanically compare the full header inventory, require ABI version 1, and exercise actual native calls on all five RIDs.
- Borrowed-pointer lifetime and callback shutdown races: copy all callback data synchronously, root callback entry points, hold userdata until callbacks are cleared and the native client is destroyed, and keep public handlers off the native worker thread.
- Native reconnect does not itself mean a connection succeeded: complete `ConnectAsync` only on the `CONNECTED` callback; fault or cancel it on terminal state, timeout, or caller cancellation.
- Native assets can be omitted from local package builds: make packaging fail if any required RID library is absent and validate the packed nupkg contents in CI.
- ARM runner availability or local platform mismatch can limit execution: CI uses the existing native target matrix and records any unavailable runner as a limitation rather than asserting coverage.

## Acceptance criteria

- Every one of the 98 public declarations has an exact Cdecl managed binding and is checked for declaration/export parity.
- All owned resources have deterministic, idempotent cleanup; callback exceptions are contained; user callbacks cannot block the native worker.
- Normalized chat, gift, like, member, social, and room-user models coexist with full schema, raw, and future-event access.
- `ConnectAsync` observes a real connected transition, and cancellation, reconnect, multiple clients, and reentrant shutdown are covered offline.
- The package contains all five native RID assets and resolves them without manual copying.
- Tests, native regression checks, examples, docs, and the requested PR are completed; no live TikTok test is claimed.
