using System.Runtime.InteropServices;
using System.Threading.Channels;
using Ttl.Live.Native;

namespace Ttl.Live;

/// <summary>Independent asynchronous client backed by one native Rust runtime and signer.</summary>
public sealed class TikTokLiveClient : IDisposable, IAsyncDisposable
{
    private static readonly AsyncLocal<bool> IsDispatching = new();
    private readonly TikTokLiveOptions _options;
    private readonly NativeClientHandle _native;
    private readonly Channel<Action> _dispatch;
    private readonly Task _dispatchTask;
    private readonly SemaphoreSlim _lifecycle = new(1, 1);
    private readonly object _disposeLock = new();
    private TaskCompletionSource? _connectionTransition;
    private Task? _disposeTask;
    private int _state;
    private int _disposeRequested;
    private string? _stateDetail;

    public event EventHandler<TikTokEventArgs>? EventReceived;
    public event EventHandler<TikTokBatchEventArgs>? BatchReceived;
    public event EventHandler<ChatReceivedEventArgs>? ChatReceived;
    public event EventHandler<GiftReceivedEventArgs>? GiftReceived;
    public event EventHandler<LikeReceivedEventArgs>? LikeReceived;
    public event EventHandler<MemberReceivedEventArgs>? MemberReceived;
    public event EventHandler<SocialReceivedEventArgs>? SocialReceived;
    public event EventHandler<RoomUserReceivedEventArgs>? RoomUserReceived;
    public event EventHandler<ConnectionStateChangedEventArgs>? StateChanged;
    public event EventHandler<NativeErrorEventArgs>? ErrorReceived;
    public event EventHandler<EventHandlerExceptionEventArgs>? HandlerException;

    public ConnectionState State => (ConnectionState)Volatile.Read(ref _state);
    public string? StateDetail => Volatile.Read(ref _stateDetail);
    public string UniqueId => _options.UniqueId;
    public static NativeLibraryInfo NativeLibrary => NativeRuntime.EnsureLoaded();

    public TikTokLiveClient(TikTokLiveOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        options.Validate();
        string bundleSource = options.ResolveBundleSource();
        NativeRuntime.EnsureLoaded();
        _options = options;
        _dispatch = Channel.CreateUnbounded<Action>(new UnboundedChannelOptions
        {
            SingleReader = true,
            SingleWriter = false,
            AllowSynchronousContinuations = false
        });
        _dispatchTask = Task.Run(() => DispatchLoopAsync(_dispatch.Reader,
            new WeakReference<TikTokLiveClient>(this)));

        using var uniqueId = new Utf8Input(options.UniqueId);
        using var bundle = new Utf8Input(bundleSource);
        using var cookies = new Utf8Input(options.CookieHeader);
        var nativeOptions = new NativeLiveOptions
        {
            StructSize = checked((uint)Marshal.SizeOf<NativeLiveOptions>()),
            UniqueId = uniqueId.Pointer,
            UniqueIdLen = uniqueId.Length,
            Bundle = bundle.Pointer,
            BundleLen = bundle.Length,
            Reconnect = options.AutoReconnect ? (byte)1 : (byte)0,
            MaxReconnectAttempts = options.MaxReconnectAttempts,
            CookieHeader = cookies.Pointer,
            CookieHeaderLen = cookies.Length
        };
        nint pointer = NativeMethods.ttl_live_client_create(in nativeOptions);
        if (pointer == 0)
        {
            _dispatch.Writer.TryComplete();
            throw NativeRuntime.LastError("create client");
        }

        _native = new NativeClientHandle(pointer, new CallbackContext(
            new WeakReference<TikTokLiveClient>(this), _dispatch.Writer));
        try
        {
            CheckRegistration(NativeMethods.ttl_live_client_set_event_callback(pointer, NativeCallbacks.EventPointer, _native.Userdata));
            CheckRegistration(NativeMethods.ttl_live_client_set_batch_callback(pointer, NativeCallbacks.BatchPointer, _native.Userdata));
            CheckRegistration(NativeMethods.ttl_live_client_set_state_callback(pointer, NativeCallbacks.StatePointer, _native.Userdata));
            CheckRegistration(NativeMethods.ttl_live_client_set_error_callback(pointer, NativeCallbacks.ErrorPointer, _native.Userdata));
        }
        catch
        {
            _native.Dispose();
            _dispatch.Writer.TryComplete();
            throw;
        }
    }

    /// <summary>Waits for a real native CONNECTED state callback, subject to timeout and cancellation.</summary>
    public async Task ConnectAsync(CancellationToken cancellationToken = default)
    {
        Task wait;
        await _lifecycle.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            ThrowIfDisposing();
            ConnectionState state = State;
            if (state == ConnectionState.Connected) return;
            if (state is ConnectionState.Connecting or ConnectionState.Reconnecting)
            {
                _connectionTransition ??= NewTransition();
                wait = _connectionTransition.Task;
            }
            else
            {
                _connectionTransition = NewTransition();
                int result = await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_connect), CancellationToken.None).ConfigureAwait(false);
                if (result != 0)
                {
                    TikTokLiveException error = NativeRuntime.LastError("connect");
                    _connectionTransition = null;
                    throw error;
                }
                wait = _connectionTransition.Task;
            }
        }
        finally { _lifecycle.Release(); }

        try
        {
            if (_options.ConnectTimeout == Timeout.InfiniteTimeSpan)
                await wait.WaitAsync(cancellationToken).ConfigureAwait(false);
            else
                await wait.WaitAsync(_options.ConnectTimeout, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is OperationCanceledException or TimeoutException)
        {
            await DisconnectAsync(CancellationToken.None).ConfigureAwait(false);
            throw;
        }
    }

    /// <summary>Stops the native worker and waits for native callbacks and shutdown to finish.</summary>
    public async Task DisconnectAsync(CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        await _lifecycle.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (_native.IsClosed || _native.IsInvalid) return;
            int result = await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_disconnect), CancellationToken.None).ConfigureAwait(false);
            if (result != 0) throw NativeRuntime.LastError("disconnect");
        }
        finally { _lifecycle.Release(); }
    }

    public ValueTask DisposeAsync()
    {
        lock (_disposeLock) _disposeTask ??= DisposeCoreAsync();
        return new ValueTask(_disposeTask);
    }

    public void Dispose() => DisposeAsync().AsTask().GetAwaiter().GetResult();

    private async Task DisposeCoreAsync()
    {
        Interlocked.Exchange(ref _disposeRequested, 1);
        await _lifecycle.WaitAsync().ConfigureAwait(false);
        try
        {
            if (!_native.IsClosed && !_native.IsInvalid)
            {
                try
                {
                    await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_disconnect), CancellationToken.None).ConfigureAwait(false);
                }
                catch { /* Destroy below remains synchronous and owns final shutdown. */ }
            }
            _connectionTransition?.TrySetException(new ObjectDisposedException(nameof(TikTokLiveClient)));
            _native.Dispose();
        }
        finally
        {
            Volatile.Write(ref _state, (int)ConnectionState.Disconnected);
            _lifecycle.Release();
            _dispatch.Writer.TryComplete();
        }

        if (!IsDispatching.Value) await _dispatchTask.ConfigureAwait(false);
    }

    internal void OnNativeEvent(nint borrowed)
    {
        if (Volatile.Read(ref _disposeRequested) != 0) return;
        NativeEventHandle owned = NativeEventHandle.Clone(borrowed);
        if (!Queue(() =>
            {
                using (owned)
                {
                    if (Volatile.Read(ref _disposeRequested) != 0) return;
                    try { PublishEvent(NativeEventReader.CaptureEvent(owned.DangerousGetHandle())); }
                    catch (Exception ex) { PublishError(new TikTokLiveException($"Could not copy native event: {ex.Message}", -1, inner: ex)); }
                }
            })) owned.Dispose();
    }

    internal void OnNativeBatch(nint borrowed)
    {
        if (Volatile.Read(ref _disposeRequested) != 0) return;
        nuint nativeCount = NativeMethods.ttl_batch_event_count(borrowed);
        if (nativeCount > 8192) throw new InvalidDataException("Native batch exceeds its documented event bound.");
        var copiedEvents = new List<NativeEventHandle>((int)nativeCount);
        try
        {
            for (nuint i = 0; i < nativeCount; i++)
            {
                nint item = NativeMethods.ttl_batch_event_at(borrowed, i);
                copiedEvents.Add(NativeEventHandle.Clone(item));
            }
            var cursor = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_cursor(borrowed)) ?? string.Empty;
            var ext = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_internal_ext(borrowed)) ?? string.Empty;
            var server = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_push_server(borrowed)) ?? string.Empty;
            bool needAck = NativeMethods.ttl_batch_need_ack(borrowed);
            long heartbeat = NativeMethods.ttl_batch_heartbeat_duration(borrowed);
            var owned = copiedEvents.ToArray();
            copiedEvents.Clear();
            if (!Queue(() =>
                {
                    try
                    {
                        if (Volatile.Read(ref _disposeRequested) != 0) return;
                        var events = owned.Select(handle => NativeEventReader.CaptureEvent(handle.DangerousGetHandle())).ToArray();
                        PublishBatch(new TtlEventBatch(Array.AsReadOnly(events), cursor, ext, server, needAck, heartbeat));
                    }
                    catch (Exception ex) { PublishError(new TikTokLiveException($"Could not copy native event batch: {ex.Message}", -1, inner: ex)); }
                    finally { foreach (var handle in owned) handle.Dispose(); }
                }))
                foreach (var handle in owned) handle.Dispose();
        }
        finally { foreach (var handle in copiedEvents) handle.Dispose(); }
    }

    internal void OnNativeState(int nativeState, string? detail)
    {
        if (!Enum.IsDefined(typeof(ConnectionState), nativeState)) return;
        var state = (ConnectionState)nativeState;
        ConnectionState previous = State;
        if (state == ConnectionState.Connecting && previous != ConnectionState.Connecting &&
            previous != ConnectionState.Reconnecting && _connectionTransition?.Task.IsCompleted == true)
            _connectionTransition = NewTransition();
        if (state == ConnectionState.Connecting && _connectionTransition is null)
            _connectionTransition = NewTransition();
        Interlocked.Exchange(ref _state, nativeState);
        Volatile.Write(ref _stateDetail, detail);

        if (state == ConnectionState.Connected)
            _connectionTransition?.TrySetResult();
        else if (state == ConnectionState.Reconnecting)
        {
            if (_connectionTransition?.Task.IsCompleted == true) _connectionTransition = NewTransition();
        }
        else if (state is ConnectionState.Offline or ConnectionState.Error)
            _connectionTransition?.TrySetException(new TikTokLiveException(detail ?? $"Connection entered {state}.", nativeState));
        else if (state == ConnectionState.Disconnected && _connectionTransition is { Task.IsCompleted: false })
            _connectionTransition.TrySetException(new TikTokLiveException(detail ?? "Connection ended before it reached CONNECTED.", nativeState));

        Queue(() =>
        {
            if (Volatile.Read(ref _disposeRequested) == 0)
                InvokeHandlers(StateChanged, new ConnectionStateChangedEventArgs(state, detail));
        });
    }

    internal void OnNativeError(TikTokLiveException error)
    {
        _connectionTransition?.TrySetException(error);
        Queue(() =>
        {
            if (Volatile.Read(ref _disposeRequested) == 0) PublishError(error);
        });
    }

    internal void OnInteropFailure(Exception error) => OnNativeError(
        new TikTokLiveException($"Native callback data could not be copied: {error.Message}", -1, inner: error));

    private void PublishEvent(TtlEvent value)
    {
        InvokeHandlers(EventReceived, new TikTokEventArgs(value));
        switch (value.Normalized)
        {
            case ChatLiveEvent chat: InvokeHandlers(ChatReceived, new ChatReceivedEventArgs(value, chat)); break;
            case GiftLiveEvent gift: InvokeHandlers(GiftReceived, new GiftReceivedEventArgs(value, gift)); break;
            case LikeLiveEvent like: InvokeHandlers(LikeReceived, new LikeReceivedEventArgs(value, like)); break;
            case MemberLiveEvent member: InvokeHandlers(MemberReceived, new MemberReceivedEventArgs(value, member)); break;
            case SocialLiveEvent social: InvokeHandlers(SocialReceived, new SocialReceivedEventArgs(value, social)); break;
            case RoomUserLiveEvent roomUser: InvokeHandlers(RoomUserReceived, new RoomUserReceivedEventArgs(value, roomUser)); break;
        }
    }

    private void PublishBatch(TtlEventBatch batch) => InvokeHandlers(BatchReceived, new TikTokBatchEventArgs(batch));
    private void PublishError(TikTokLiveException error) => InvokeHandlers(ErrorReceived, new NativeErrorEventArgs(error));

    private static async Task DispatchLoopAsync(ChannelReader<Action> reader,
        WeakReference<TikTokLiveClient> owner)
    {
        while (await reader.WaitToReadAsync().ConfigureAwait(false))
        {
            while (reader.TryRead(out Action? work))
            {
                IsDispatching.Value = true;
                try { work(); }
                catch (Exception ex)
                {
                    if (owner.TryGetTarget(out TikTokLiveClient? client)) client.NotifyHandlerException(ex);
                }
                finally
                {
                    IsDispatching.Value = false;
                    work = null;
                }
            }
        }
    }

    private void InvokeHandlers<T>(EventHandler<T>? handlers, T args) where T : EventArgs
    {
        if (handlers is null) return;
        foreach (EventHandler<T> handler in handlers.GetInvocationList())
        {
            try { handler(this, args); }
            catch (Exception ex) { NotifyHandlerException(ex); }
        }
    }

    private void NotifyHandlerException(Exception exception)
    {
        var handlers = HandlerException;
        if (handlers is null) return;
        foreach (EventHandler<EventHandlerExceptionEventArgs> handler in handlers.GetInvocationList())
        {
            try { handler(this, new EventHandlerExceptionEventArgs(exception)); }
            catch { }
        }
    }

    private bool Queue(Action work)
    {
        if (Volatile.Read(ref _disposeRequested) != 0) return false;
        return _dispatch.Writer.TryWrite(work);
    }

    private static TaskCompletionSource NewTransition() =>
        new(TaskCreationOptions.RunContinuationsAsynchronously);

    private void ThrowIfDisposing()
    {
        if (Volatile.Read(ref _disposeRequested) != 0)
            throw new ObjectDisposedException(nameof(TikTokLiveClient));
    }

    private int InvokeClient(Func<nint, int> operation)
    {
        bool added = false;
        try
        {
            _native.DangerousAddRef(ref added);
            if (_native.IsInvalid || _native.IsClosed) throw new ObjectDisposedException(nameof(TikTokLiveClient));
            return operation(_native.DangerousGetHandle());
        }
        finally
        {
            if (added) _native.DangerousRelease();
        }
    }

    private static void CheckRegistration(int result)
    {
        if (result != 0) throw NativeRuntime.LastError("register callback");
    }
}
