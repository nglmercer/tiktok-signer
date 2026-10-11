# Ttl.Live for .NET

`Ttl.Live` is a .NET 8 and .NET 10 SDK for consuming TikTok LIVE events. Networking, signing, protobuf decoding, and schema discovery are provided by the Rust `ttl-live-native` library through its public C ABI.

## Install

The NuGet package is designed to include these native runtime assets, selected automatically by NuGet:

| RID | Native library |
| --- | --- |
| `win-x64` | `ttl_live.dll` |
| `linux-x64` | `libttl_live.so` |
| `linux-arm64` | `libttl_live.so` |
| `osx-x64` | `libttl_live.dylib` |
| `osx-arm64` | `libttl_live.dylib` |

After the package is published, install it with:

```sh
dotnet add package Ttl.Live
```

For source builds, the native library must be built for the current RID. Set `TTL_LIVE_NATIVE_LIBRARY` to an explicit library path when testing a local native build. The library ABI version is checked at startup and incompatible libraries fail with a diagnostic that reports the expected and detected ABI versions.

`ttl-live-native` currently requires the source of a trusted JavaScript signing bundle. Set `TTL_BUNDLE` to its local file path, set `TikTokLiveOptions.BundlePath`, or pass the source in `TikTokLiveOptions.Bundle`. The repository's `./live` helper fetches the pinned public bundle and verifies its hash; the SDK does not download or reimplement signing.

## Connect and handle events

```csharp
using Ttl.Live;

await using var client = new TikTokLiveClient(
    new TikTokLiveOptions
    {
        UniqueId = "@creator",
        AutoReconnect = true
    });

client.ChatReceived += (_, e) => Console.WriteLine(e.Comment);
client.GiftReceived += (_, e) => Console.WriteLine(e.GiftName);
client.EventReceived += (_, e) => Console.WriteLine(e.Event.Method);

await client.ConnectAsync();
```

`ConnectAsync` completes only after the native state callback reports `Connected`. Concurrent callers share one connection attempt. Each caller's cancellation token cancels only that caller's wait; `ConnectTimeout` (60 seconds by default) bounds the shared attempt and stops the native worker when it expires. `DisconnectAsync`, `Dispose`, and `DisposeAsync` are repeat-safe, and a disconnected client can connect again. Each client owns an independent native worker and signer.

## Typed and generic events

The client exposes `ChatReceived`, `GiftReceived`, `LikeReceived`, `MemberReceived`, `SocialReceived`, and `RoomUserReceived`. Every event also arrives at `EventReceived`; events from future native versions remain accessible by original method name and are not mapped to a closed event enum. Subscribe to `BatchReceived` for decoded batch metadata and events.

Each `TtlEvent` preserves method and method ID, message ID, timestamp when present, history flag, native support kind, schema name, flags, raw payload, and (by default) the complete copied protobuf field tree. JSON modes are serialized only when first requested and then cached. Chat and gift typed values use native convenience views; other normalized types materialize only normalized JSON. Set `CaptureFullFieldTree = false` when raw payload and normalized values are sufficient. `Root.Fields` preserves wire order and duplicate occurrences when full capture is enabled. `TtlObject.Find`, `FindAll`, `MapEntries`, and `ActiveOneof` support schema-guided traversal. Enum numbers are retained when a descriptor does not know the enum name. `TikTokSchemaCatalog.Events`, `Find`, and `Fingerprint` expose descriptor introspection.

```csharp
client.EventReceived += (_, args) =>
{
    TtlEvent item = args.Event;
    Console.WriteLine($"{item.Method} id={item.MessageId} support={item.Support}");
    Console.WriteLine(item.ToJson(JsonMode.Full));
    foreach (TtlField field in item.Root.Fields)
        Console.WriteLine($"#{field.Number} {field.Name ?? "<unknown>"}: {field.Value}");
};

client.BatchReceived += (_, args) =>
    Console.WriteLine($"batch count={args.Batch.Events.Count} ack={args.Batch.NeedAck}");
```

`JsonMode` values are `Normalized`, `Schema`, `Full`, `Compact`, and `Raw`; normalized JSON is `null` when the native normalizer does not support that method. Full and raw views retain unknown wire information. The decoder is also available offline through `TikTokEventDecoder.Decode` and `DecodeBatch`.

## Errors and callback threading

`ErrorReceived` exposes asynchronous native failures with the native code, retryability, retry-after duration, and HTTP status when supplied. `TikTokLiveException` is thrown for synchronous native failures. `NativeLibraryException` reports missing, incompatible, or unloadable native libraries. `StateChanged` reports native state transitions. User event handlers run serially on a managed dispatcher, never on the native worker thread. Handler exceptions are contained and reported through `HandlerException`.

The dispatcher is bounded per client. `EventQueueCapacity` limits pending callback work items, `EventQueueByteLimit` limits estimated bytes for queued and currently processed events, `MaxBatchEvents` bounds batch copies, and `EventOverflowPolicy` selects `DropNewest` (default) or `DropOldest`. Native callbacks never wait for a slow handler. `client.Diagnostics` reports queue depth/bytes, dropped events/work, and processing failures. A batch callback is queued once; when the native ABI follows it with individual callbacks for the same batch members, the SDK decodes each member once, delivers `BatchReceived` first, then `EventReceived` and typed events in batch order.

## Multiple clients

Create one `TikTokLiveClient` per creator. Each client has independent native networking and signing state:

```csharp
await using var first = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "creator_one" });
await using var second = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "creator_two" });
first.EventReceived += (_, e) => Console.WriteLine($"one: {e.Event.Method}");
second.EventReceived += (_, e) => Console.WriteLine($"two: {e.Event.Method}");
await Task.WhenAll(first.ConnectAsync(), second.ConnectAsync());
```

## Runnable examples

Set `TTL_BUNDLE` to the trusted bundle file path and `TTL_LIVE_NATIVE_LIBRARY` only for local native builds. Run any console example with a creator ID:

```sh
dotnet run --project sdk/dotnet/examples/BasicConsole -- @creator
dotnet run --project sdk/dotnet/examples/TypedAndGeneric -- @creator
dotnet run --project sdk/dotnet/examples/ChatAndGifts -- @creator
dotnet run --project sdk/dotnet/examples/MultipleClients -- creator_one creator_two
dotnet run --project sdk/dotnet/examples/SchemaInspector -- @creator
```

The relay runs as an ASP.NET Core service. Set `LIVE_CREATORS` to a comma-separated list, configure the bundle path, and host it behind HTTPS:

```sh
LIVE_CREATORS=creator_one,creator_two TTL_BUNDLE=/secure/path/webmssdk.js \
  dotnet run --project sdk/dotnet/examples/AspNetCoreRelay
```

The relay exposes `/events` as server-sent events containing native full-mode JSON, `/health` for status, and `/vrchat/events` as a finite JSON snapshot. Each SSE connection has its own bounded queue; slow subscribers lose oldest queued messages without blocking other clients. The snapshot keeps bounded chat/gift history and uses monotonic sequence IDs (`id`) so polling clients can deduplicate. It includes gift ID, repeat/combo counts, group ID, repeat-end status, and diamond count. User strings are JSON encoded, the response is capped, and the endpoint returns only public event fields—not cookies or signing bundles.

Configure `LIVE_SSE_QUEUE_CAPACITY` (default `64`), `VRCHAT_HISTORY_EVENTS` (default `256`), and `VRCHAT_MAX_PAYLOAD_BYTES` (default `65536`). Set `VRCHAT_ALLOWED_IPS` to a comma-separated list of exact peer IP addresses to restrict `/vrchat/events`; when unset, the read-only snapshot endpoint is public. The allowlist uses the direct socket peer and does not trust forwarded headers. A fixed `/vrchat/events` URL works with polling intervals of five seconds or longer; clients should retain the highest sequence and ignore IDs they have already applied. Keep SSE and snapshot access behind HTTPS and any deployment-specific ingress controls.

## ASP.NET Core relay example

The `examples/AspNetCoreRelay` project shows a hosted service consuming LIVE events and exposing broadcast SSE plus the finite VRChat snapshot endpoint. Deploy it on a host where the matching native runtime is available, terminate HTTPS at the service or a trusted reverse proxy, and set server-side access controls appropriate to the relay. Do not put privileged service credentials in an uploaded world; the snapshot route is read-only and contains no signing/session material.

## VRChat compatibility

Published VRChat worlds use Udon/UdonSharp. UdonSharp cannot directly execute this native library or unrestricted .NET P/Invoke. This SDK can power an external hosted HTTPS relay. A separately implemented VRChat world package can consume relay data through supported Udon APIs. This SDK alone does not enable direct TikTok connectivity inside uploaded VRChat worlds.

## Build and package

The native workflow builds the five RID libraries and assembles them under the NuGet `runtimes/{rid}/native/` layout. Local `dotnet pack` builds the managed package; the complete runtime package is assembled in CI after all native target artifacts are available. Publishing is not performed by CI.

## License and notices

The NuGet package is a composite of components with different terms and is not MIT-only. Package metadata points to `PACKAGE-LICENSE.txt`; the package also includes the native runtime's MIT notice and the full modified AGPL-3.0 protobuf license with its additional permissions and restrictions. Section 19 describes conditions for commercial, closed-source, or hosted network services; read the included notices before redistributing or operating a relay.
