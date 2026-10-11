using Ttl.Live.Native;
using System.Collections.Concurrent;
using System.Text.Json;

namespace Ttl.Live.Tests;

public sealed class ClientQueueTests
{
    private const string MockBundle = "window.byted_acrawler={init:function(){},registerWsSigner:function(){return function(){return {'X-Gnarly':'test-only'}}}};";

    [Fact]
    public async Task TenThousandEventsStayBoundedAndOverflowIsObservable()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "queue-stress-test",
            Bundle = MockBundle,
            EventQueueCapacity = 4,
            EventOverflowPolicy = EventOverflowPolicy.DropNewest
        });

        using var method = new Utf8Input("WebcastFutureMessage");
        using var payload = PinnedInput.From(new byte[] { 8, 42 });
        nint pointer = NativeMethods.ttl_event_decode(method.Pointer, method.Length,
            7, false, payload.Pointer, payload.Length);
        Assert.NotEqual(0, pointer);
        using var nativeEvent = new NativeEventHandle(pointer);

        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        int delivered = 0;
        client.EventReceived += (_, _) =>
        {
            if (Interlocked.Increment(ref delivered) == 1)
            {
                entered.Set();
                release.Wait(TimeSpan.FromSeconds(10));
            }
        };

        client.OnNativeEvent(nativeEvent.DangerousGetHandle());
        Assert.True(entered.Wait(TimeSpan.FromSeconds(5)));
        for (int i = 0; i < 10_000; i++) client.OnNativeEvent(nativeEvent.DangerousGetHandle());

        TikTokLiveDiagnostics whileBlocked = client.Diagnostics;
        Assert.InRange(whileBlocked.QueueDepth, 0, 4);
        Assert.Equal(4, whileBlocked.QueueDepth);
        Assert.InRange(whileBlocked.QueuedBytes, 0, whileBlocked.QueueByteLimit);
        Assert.Equal(9_996, whileBlocked.DroppedEvents);

        release.Set();
        await WaitUntilAsync(() => Volatile.Read(ref delivered) == 5);
        Assert.Equal(9_996, client.Diagnostics.DroppedEvents);
    }

    [Fact]
    public async Task DropOldestKeepsAcceptedEventsInOrderAndDisposesEvictedHandles()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "drop-oldest-test",
            Bundle = MockBundle,
            EventQueueCapacity = 2,
            EventOverflowPolicy = EventOverflowPolicy.DropOldest
        });
        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        var ids = new ConcurrentQueue<ulong>();
        client.EventReceived += (_, args) =>
        {
            ids.Enqueue(args.Event.MessageId);
            if (args.Event.MessageId == 0)
            {
                entered.Set();
                release.Wait(TimeSpan.FromSeconds(10));
            }
        };

        NativeEventHandle[] events = Enumerable.Range(0, 5).Select(CreateNativeEvent).ToArray();
        try
        {
            client.OnNativeEvent(events[0].DangerousGetHandle());
            Assert.True(entered.Wait(TimeSpan.FromSeconds(5)));
            for (int i = 1; i < events.Length; i++) client.OnNativeEvent(events[i].DangerousGetHandle());
            Assert.Equal(2, client.Diagnostics.QueueDepth);
            Assert.Equal(2, client.Diagnostics.DroppedEvents);
            release.Set();
            await WaitUntilAsync(() => ids.Count == 3);
            Assert.Equal(new ulong[] { 0, 3, 4 }, ids.ToArray());
        }
        finally
        {
            release.Set();
            foreach (NativeEventHandle item in events) item.Dispose();
        }
    }

    [Fact]
    public async Task ByteBudgetRejectsAnOversizedEventBeforeCloningIt()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "queue-byte-budget-test",
            Bundle = MockBundle,
            EventQueueCapacity = 16,
            EventQueueByteLimit = 1_048_576
        });
        client.EventReceived += (_, _) => { };
        using var method = new Utf8Input("WebcastFutureMessage");
        using var payload = PinnedInput.From(new byte[1_000_000]);
        nint pointer = NativeMethods.ttl_event_decode(method.Pointer, method.Length,
            8, false, payload.Pointer, payload.Length);
        Assert.NotEqual(0, pointer);
        using var nativeEvent = new NativeEventHandle(pointer);

        client.OnNativeEvent(nativeEvent.DangerousGetHandle());
        Assert.Equal(1, client.Diagnostics.DroppedEvents);
        Assert.Equal(0, client.Diagnostics.QueuedBytes);
        Assert.Equal(0, client.Diagnostics.QueueDepth);
    }

    [Fact]
    public async Task DisposeDrainsQueuedNativeHandlesAndByteReservations()
    {
        var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "dispose-queued-work-test",
            Bundle = MockBundle,
            EventQueueCapacity = 4,
            EventOverflowPolicy = EventOverflowPolicy.DropNewest
        });
        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        int delivered = 0;
        client.EventReceived += (_, _) =>
        {
            if (Interlocked.Increment(ref delivered) == 1)
            {
                entered.Set();
                release.Wait(TimeSpan.FromSeconds(10));
            }
        };

        NativeEventHandle[] events = Enumerable.Range(0, 5).Select(CreateNativeEvent).ToArray();
        try
        {
            client.OnNativeEvent(events[0].DangerousGetHandle());
            Assert.True(entered.Wait(TimeSpan.FromSeconds(5)));
            for (int i = 1; i < events.Length; i++) client.OnNativeEvent(events[i].DangerousGetHandle());
            Assert.Equal(4, client.Diagnostics.QueueDepth);

            Task disposal = client.DisposeAsync().AsTask();
            release.Set();
            await disposal.WaitAsync(TimeSpan.FromSeconds(5));

            Assert.Equal(0, client.Diagnostics.QueueDepth);
            Assert.Equal(0, client.Diagnostics.QueuedBytes);
            Assert.Equal(4, client.Diagnostics.DroppedEvents);
            Assert.Equal(1, Volatile.Read(ref delivered));
        }
        finally
        {
            release.Set();
            foreach (NativeEventHandle item in events) item.Dispose();
            await client.DisposeAsync();
        }
    }

    [Fact]
    public async Task DisconnectClearsBatchCallbackSuppressionState()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "batch-disconnect-cleanup-test",
            Bundle = MockBundle
        });
        client.BatchReceived += (_, _) => { };
        byte[] batchBytes = File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "fixtures", "events", "batch.pb"));
        using var input = PinnedInput.From(batchBytes);
        nint pointer = NativeMethods.ttl_batch_decode(input.Pointer, input.Length);
        Assert.NotEqual(0, pointer);
        using var batch = new NativeBatchHandle(pointer);

        client.OnNativeBatch(batch.DangerousGetHandle());
        Assert.True(client.SuppressedBatchEventCountForTests > 0);

        await client.DisconnectAsync();

        Assert.Equal(0, client.SuppressedBatchEventCountForTests);
    }

    [Fact]
    public async Task TypedOnlyCaptureKeepsRawAndLazyJsonWithoutMaterializingTheFieldTree()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "typed-only-test",
            Bundle = MockBundle,
            CaptureFullFieldTree = false
        });
        var delivered = new TaskCompletionSource<TtlEvent>(TaskCreationOptions.RunContinuationsAsynchronously);
        client.EventReceived += (_, args) => delivered.TrySetResult(args.Event);
        byte[] fixture = File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "fixtures", "events", "chat.pb"));
        using var method = new Utf8Input("WebcastChatMessage");
        using var payload = PinnedInput.From(fixture);
        nint pointer = NativeMethods.ttl_event_decode(method.Pointer, method.Length,
            9, false, payload.Pointer, payload.Length);
        Assert.NotEqual(0, pointer);
        using var nativeEvent = new NativeEventHandle(pointer);

        client.OnNativeEvent(nativeEvent.DangerousGetHandle());
        TtlEvent captured = await delivered.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Empty(captured.Root.Fields);
        Assert.False(captured.RawPayload.IsEmpty);
        Assert.IsType<ChatLiveEvent>(captured.Normalized);
        using JsonDocument full = JsonDocument.Parse(captured.ToJson(JsonMode.Full)!);
        Assert.Equal("WebcastChatMessage", full.RootElement.GetProperty("method").GetString());
    }

    private static NativeEventHandle CreateNativeEvent(int messageId)
    {
        using var method = new Utf8Input("WebcastFutureMessage");
        using var payload = PinnedInput.From(new byte[] { 8, 42 });
        nint pointer = NativeMethods.ttl_event_decode(method.Pointer, method.Length,
            (ulong)messageId, false, payload.Pointer, payload.Length);
        Assert.NotEqual(0, pointer);
        return new NativeEventHandle(pointer);
    }

    private static async Task WaitUntilAsync(Func<bool> condition)
    {
        DateTime deadline = DateTime.UtcNow.AddSeconds(5);
        while (!condition() && DateTime.UtcNow < deadline) await Task.Delay(10);
        Assert.True(condition(), "Timed out waiting for callback dispatch.");
    }
}
