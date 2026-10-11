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
        first.ChatReceived += (_, args) => chatDelivered.TrySetResult(args);
        first.EventReceived += (_, args) => genericDelivered.TrySetResult(args);
        first.BatchReceived += (_, args) => batchDelivered.TrySetResult(args);

        using var chat = DecodeNative("WebcastChatMessage", File.ReadAllBytes(Fixture("chat.pb")));
        first.OnNativeEvent(chat.DangerousGetHandle());
        chat.Dispose();
        Assert.Equal("ese edificio de que es?", (await chatDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Comment);
        Assert.Equal("WebcastChatMessage", (await genericDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Event.Method);

        byte[] batchPayload = File.ReadAllBytes(Fixture("batch.pb"));
        using var batchInput = PinnedInput.From(batchPayload);
        nint batchPointer = NativeMethods.ttl_batch_decode(batchInput.Pointer, batchInput.Length);
        Assert.NotEqual(0, batchPointer);
        using var batch = new NativeBatchHandle(batchPointer);
        first.OnNativeBatch(batch.DangerousGetHandle());
        Assert.NotEmpty((await batchDelivered.Task.WaitAsync(TimeSpan.FromSeconds(5))).Batch.Events);

        Assert.NotEqual(first.UniqueId, second.UniqueId);
        Assert.Equal(ConnectionState.Disconnected, first.State);
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
        client.OnNativeState((int)ConnectionState.Connecting, "starting");
        Task connecting = client.ConnectAsync();
        client.OnNativeState((int)ConnectionState.Connected, "connected");
        await connecting.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Equal(ConnectionState.Connected, client.State);

        client.OnNativeState((int)ConnectionState.Reconnecting, "reconnecting");
        Task reconnected = client.ConnectAsync();
        client.OnNativeState((int)ConnectionState.Connected, "restored");
        await reconnected.WaitAsync(TimeSpan.FromSeconds(2));

        client.OnNativeState((int)ConnectionState.Connecting, "pending");
        using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(30));
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.ConnectAsync(cancellation.Token));
        await client.DisconnectAsync();
        await client.DisconnectAsync();
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
}
