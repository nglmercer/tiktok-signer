using System.Collections.Concurrent;
using Ttl.Live.Native;
using System.Runtime.InteropServices;

namespace Ttl.Live.Tests;

public sealed class ClientLifecycleTests
{
    private const string MockBundle = "window.byted_acrawler={init:function(){},registerWsSigner:function(){return function(){return {'X-Gnarly':'test-only'}}}};";
    private static string Fixture(string name) => Path.Combine(AppContext.BaseDirectory, "fixtures", "events", name);

    [Fact]
    public async Task MultipleClientsDispatchGenericTypedAndBatchEventsOffNativeBoundary()
    {
        await using var first = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "creator-one", Bundle = MockBundle });
        await using var second = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "creator-two", Bundle = MockBundle });
        var chatDelivered = new TaskCompletionSource<ChatReceivedEventArgs>(TaskCreationOptions.RunContinuationsAsynchronously);
        var genericDelivered = new TaskCompletionSource<TikTokEventArgs>(TaskCreationOptions.RunContinuationsAsynchronously);
        var batchDelivered = new TaskCompletionSource<TikTokBatchEventArgs>(TaskCreationOptions.RunContinuationsAsynchronously);
        var deliveryOrder = new ConcurrentQueue<string>();
        int genericCount = 0;
        first.ChatReceived += (_, args) => chatDelivered.TrySetResult(args);
        first.EventReceived += (_, args) =>
        {
            Interlocked.Increment(ref genericCount);
            deliveryOrder.Enqueue($"event:{args.Event.MessageId}");
            genericDelivered.TrySetResult(args);
        };
        first.BatchReceived += (_, args) =>
        {
            deliveryOrder.Enqueue("batch");
            batchDelivered.TrySetResult(args);
        };

        using var chat = DecodeNative("WebcastChatMessage", File.ReadAllBytes(Fixture("chat.pb")));
        first.OnNativeEvent(chat.DangerousGetHandle());
        chat.Dispose();
        Assert.Equal("ese edificio de que es?", (await chatDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Comment);
        Assert.Equal("WebcastChatMessage", (await genericDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Event.Method);
        while (deliveryOrder.TryDequeue(out _)) { }

        byte[] batchPayload = File.ReadAllBytes(Fixture("batch.pb"));
        using var batchInput = PinnedInput.From(batchPayload);
        nint batchPointer = NativeMethods.ttl_batch_decode(batchInput.Pointer, batchInput.Length);
        Assert.NotEqual(0, batchPointer);
        using var batch = new NativeBatchHandle(batchPointer);
        first.OnNativeBatch(batch.DangerousGetHandle());
        nuint nativeCount = NativeMethods.ttl_batch_event_count(batch.DangerousGetHandle());
        for (nuint i = 0; i < nativeCount; i++)
            first.OnNativeEvent(NativeMethods.ttl_batch_event_at(batch.DangerousGetHandle(), i));
        TtlEventBatch capturedBatch = (await batchDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Batch;
        Assert.NotEmpty(capturedBatch.Events);
        await WaitUntilAsync(() => Volatile.Read(ref genericCount) == 1 + capturedBatch.Events.Count);
        Assert.Equal(new[] { "batch" }.Concat(capturedBatch.Events.Select(item => $"event:{item.MessageId}")),
            deliveryOrder.ToArray());

        Assert.NotEqual(first.UniqueId, second.UniqueId);
        Assert.Equal(ConnectionState.Disconnected, first.State);
    }

    [Fact]
    public async Task ConcurrentConnectCallersShareOneNativeStateTransition()
    {
        await using var client = NewClient("concurrent-connect-test");
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests,
            (int)ConnectionState.Connecting, "starting shared attempt");
        using var start = new ManualResetEventSlim();
        Task[] callers = Enumerable.Range(0, 32).Select(_ => Task.Run(async () =>
        {
            start.Wait();
            await client.ConnectAsync();
        })).ToArray();

        start.Set();
        await Task.Delay(25);
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests,
            (int)ConnectionState.Connected, "shared attempt connected");
        await Task.WhenAll(callers).WaitAsync(TimeSpan.FromSeconds(5));

        Assert.Equal(ConnectionState.Connected, client.State);
    }

    [Fact]
    public async Task NativeHandlerExceptionsAreContainedAndReentrantDisposeDoesNotDeadlock()
    {
        var client = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "callback-test", Bundle = MockBundle });
        var handlerFailure = new TaskCompletionSource<Exception>(TaskCreationOptions.RunContinuationsAsynchronously);
        var disposed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        client.HandlerException += (_, args) => handlerFailure.TrySetResult(args.Exception);
        client.EventReceived += (_, _) => throw new InvalidOperationException("fixture handler failure");
        client.GiftReceived += (_, _) =>
        {
            client.Dispose();
            disposed.TrySetResult();
        };

        using var gift = DecodeNative("WebcastGiftMessage", File.ReadAllBytes(Fixture("gift.pb")));
        client.OnNativeEvent(gift.DangerousGetHandle());
        gift.Dispose();
        await disposed.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.IsType<InvalidOperationException>(await handlerFailure.Task.WaitAsync(TimeSpan.FromSeconds(5)));
        await client.DisposeAsync();
        Assert.Equal(ConnectionState.Disconnected, client.State);
    }

    [Fact]
    public async Task StateTransitionsCancellationAndReconnectAreObservedFromCallbacks()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "state-test",
            Bundle = MockBundle,
            ConnectTimeout = TimeSpan.FromSeconds(5)
        });
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "starting");
        Task connecting = client.ConnectAsync();
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connected, "connected");
        await connecting.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Equal(ConnectionState.Connected, client.State);

        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Reconnecting, "reconnecting");
        Task reconnected = client.ConnectAsync();
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connected, "restored");
        await reconnected.WaitAsync(TimeSpan.FromSeconds(2));

        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "pending");
        using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(30));
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.ConnectAsync(cancellation.Token));
        await client.DisconnectAsync();
        await client.DisconnectAsync();
    }

    [Fact]
    public async Task ExplicitDisconnectAllowsReconnectAndCancelsOnlyPendingWaiters()
    {
        await using var client = NewClient("reconnect-test");
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "first attempt");
        using var firstWaitCancellation = new CancellationTokenSource();
        Task canceledWaiter = client.ConnectAsync(firstWaitCancellation.Token);
        Task sharedWaiter = client.ConnectAsync();
        firstWaitCancellation.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => canceledWaiter);
        Assert.False(sharedWaiter.IsCompleted);
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connected, "first connected");
        await sharedWaiter.WaitAsync(TimeSpan.FromSeconds(2));

        await client.DisconnectAsync();
        Assert.Equal(ConnectionState.Disconnected, client.State);

        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "second attempt");
        Task reconnect = client.ConnectAsync();
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connected, "second connected");
        await reconnect.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Equal(ConnectionState.Connected, client.State);
    }

    [Fact]
    public async Task DisconnectAndTerminalStatesCompletePendingConnectTasks()
    {
        await using var client = NewClient("pending-test");
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "pending");
        Task pending = client.ConnectAsync();
        await client.DisconnectAsync();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending);

        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "offline attempt");
        Task offline = client.ConnectAsync();
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Offline, "creator is offline");
        TikTokLiveException error = await Assert.ThrowsAsync<TikTokLiveException>(() => offline);
        Assert.Contains("offline", error.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task RetryableErrorsKeepTheSharedTransitionAliveAcrossReconnecting()
    {
        await using var client = NewClient("retry-state-test");
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "starting");
        Task pending = client.ConnectAsync();
        NativeCallbacks.InvokeErrorCallbackForTests(client.NativeUserdataForTests,
            (int)NativeResultCode.Socket, "temporary socket failure", retryable: true);
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Reconnecting, "retrying");
        Assert.False(pending.IsCompleted);
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connected, "restored");
        await pending.WaitAsync(TimeSpan.FromSeconds(2));
    }

    [Fact]
    public async Task SharedConnectionTimeoutDisconnectsTheAttempt()
    {
        await using var client = new TikTokLiveClient(new TikTokLiveOptions
        {
            UniqueId = "timeout-test",
            Bundle = MockBundle,
            ConnectTimeout = TimeSpan.FromMilliseconds(50)
        });
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "pending");
        await Assert.ThrowsAsync<TimeoutException>(() => client.ConnectAsync());
        Assert.Equal(ConnectionState.Disconnected, client.State);
    }

    [Fact]
    public async Task DisposeCompletesPendingTransitionAndIsIdempotent()
    {
        var client = NewClient("dispose-pending-test");
        NativeCallbacks.InvokeStateCallbackForTests(client.NativeUserdataForTests, (int)ConnectionState.Connecting, "pending");
        Task pending = client.ConnectAsync();
        await client.DisposeAsync();
        await Assert.ThrowsAsync<ObjectDisposedException>(() => pending);
        await client.DisposeAsync();
        client.Dispose();
    }

    [Fact]
    public async Task DoubleDisposeAndAlreadyCancelledConnectAreSafe()
    {
        var client = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = "dispose-test", Bundle = MockBundle });
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.ConnectAsync(cancellation.Token));
        await client.DisposeAsync();
        await client.DisposeAsync();
        client.Dispose();
    }

    private static NativeEventHandle DecodeNative(string method, byte[] payload)
    {
        using var input = new Utf8Input(method);
        nint ptr = CopyPointer(payload, out nint memory);
        try
        {
            nint eventPointer = NativeMethods.ttl_event_decode(input.Pointer, input.Length, 1,
                false, ptr, (nuint)payload.Length);
            if (eventPointer == 0) throw NativeRuntime.LastError("create test event");
            return new NativeEventHandle(eventPointer);
        }
        finally { Marshal.FreeHGlobal(memory); }
    }

    private static nint CopyPointer(byte[] bytes, out nint memory)
    {
        memory = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, memory, bytes.Length);
        return memory;
    }

    private static TikTokLiveClient NewClient(string id) => new(new TikTokLiveOptions
    {
        UniqueId = id,
        Bundle = MockBundle,
        ConnectTimeout = TimeSpan.FromSeconds(5)
    });

    private static async Task WaitUntilAsync(Func<bool> condition)
    {
        DateTime deadline = DateTime.UtcNow.AddSeconds(5);
        while (!condition() && DateTime.UtcNow < deadline)
            await Task.Delay(10);
        Assert.True(condition(), "Timed out waiting for queued callback work.");
    }
}
