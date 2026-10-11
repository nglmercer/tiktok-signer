using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading.Channels;
using Ttl.Live.Native;

namespace Ttl.Live;

/// <summary>Independent asynchronous client backed by one native Rust runtime and signer.</summary>
public sealed class TikTokLiveClient : IDisposable, IAsyncDisposable
{
    private static readonly AsyncLocal<TikTokLiveClient?> DispatchOwner = new();
    private readonly TikTokLiveOptions _options;
    private readonly NativeClientHandle _native;
    private readonly Channel<DispatchWork> _dispatch;
    private readonly Task _dispatchTask;
    private readonly SemaphoreSlim _lifecycle = new(1, 1);
    private readonly object _disposeLock = new();
    private readonly object _stateLock = new();
    private readonly object _suppressionLock = new();
    private readonly HashSet<nint> _suppressedBatchEvents = [];
    private TaskCompletionSource? _connectionTransition;
    private long _transitionVersion;
    private Task? _disposeTask;
    private int _state;
    private int _disposeRequested;
    private bool _disconnecting;
    private string? _stateDetail;
    private long _droppedEvents;
    private long _droppedWorkItems;
    private long _processingFailures;
    private long _queuedBytes;

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
    internal nint NativeUserdataForTests => _native.Userdata;
    internal int SuppressedBatchEventCountForTests
    {
        get { lock (_suppressionLock) return _suppressedBatchEvents.Count; }
    }
    public TikTokLiveDiagnostics Diagnostics => new(
        ReadQueueDepth(), _options.EventQueueCapacity, Interlocked.Read(ref _queuedBytes), _options.EventQueueByteLimit,
        Interlocked.Read(ref _droppedEvents), Interlocked.Read(ref _droppedWorkItems),
        Interlocked.Read(ref _processingFailures));
    public static NativeLibraryInfo NativeLibrary => NativeRuntime.EnsureLoaded();

    public TikTokLiveClient(TikTokLiveOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        options.Validate();
        string bundleSource = options.ResolveBundleSource();
        NativeRuntime.EnsureLoaded();
        _options = options;
        var channelOptions = new BoundedChannelOptions(options.EventQueueCapacity)
        {
            SingleReader = true,
            SingleWriter = false,
            AllowSynchronousContinuations = false,
            FullMode = options.EventOverflowPolicy == EventOverflowPolicy.DropOldest
                ? BoundedChannelFullMode.DropOldest
                : BoundedChannelFullMode.DropWrite
        };
        _dispatch = Channel.CreateBounded<DispatchWork>(channelOptions, DropDispatchedWork);
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
            new WeakReference<TikTokLiveClient>(this)));
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

    /// <summary>
    /// Starts one native connection attempt and waits for CONNECTED. Each caller's cancellation token
    /// cancels only that caller's wait; it never disconnects an attempt shared with other callers.
    /// The configured ConnectTimeout applies once to the underlying transition and disconnects it.
    /// </summary>
    public async Task ConnectAsync(CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        Task wait;
        long transitionVersion;
        bool startNativeAttempt = false;

        await _lifecycle.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            ThrowIfDisposing();
            lock (_stateLock)
            {
                ConnectionState state = (ConnectionState)_state;
                if (state == ConnectionState.Connected) return;

                if (state is ConnectionState.Connecting or ConnectionState.Reconnecting)
                {
                    if (_connectionTransition is null || _connectionTransition.Task.IsCompleted)
                    {
                        _connectionTransition = NewTransition();
                        _transitionVersion++;
                        if (state == ConnectionState.Connecting) startNativeAttempt = true;
                        else StartTransitionTimeout(_transitionVersion, _connectionTransition.Task);
                    }
                }
                else
                {
                    _connectionTransition = NewTransition();
                    _transitionVersion++;
                    _disconnecting = false;
                    Volatile.Write(ref _state, (int)ConnectionState.Connecting);
                    Volatile.Write(ref _stateDetail, null);
                    startNativeAttempt = true;
                }

                wait = _connectionTransition.Task;
                transitionVersion = _transitionVersion;
            }

            if (startNativeAttempt)
            {
                int result;
                try
                {
                    result = await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_connect),
                        CancellationToken.None).ConfigureAwait(false);
                }
                catch (Exception error)
                {
                    FailTransition(transitionVersion, error, ConnectionState.Error, error.Message);
                    if (error is TikTokLiveException nativeError) QueueError(nativeError);
                    throw;
                }
                if (result != 0)
                {
                    TikTokLiveException error = NativeRuntime.LastError("connect");
                    FailTransition(transitionVersion, error, ConnectionState.Error, error.Message);
                    QueueError(error);
                    throw error;
                }
                StartTransitionTimeout(transitionVersion, wait);
            }
        }
        finally { _lifecycle.Release(); }

        await wait.WaitAsync(cancellationToken).ConfigureAwait(false);
    }

    /// <summary>Stops the native worker and waits for native callbacks and shutdown to finish.</summary>
    public async Task DisconnectAsync(CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        await _lifecycle.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (Volatile.Read(ref _disposeRequested) != 0 || _native.IsClosed || _native.IsInvalid) return;

            ConnectionState previous;
            lock (_stateLock)
            {
                previous = (ConnectionState)_state;
                _disconnecting = true;
                _transitionVersion++;
                _connectionTransition?.TrySetCanceled();
            }

            // Once shutdown begins the operation is allowed to finish even if the caller later cancels.
            int result;
            try
            {
                result = await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_disconnect),
                    CancellationToken.None).ConfigureAwait(false);
            }
            catch
            {
                lock (_stateLock) _disconnecting = false;
                throw;
            }
            if (result != 0)
            {
                lock (_stateLock) _disconnecting = false;
                throw NativeRuntime.LastError("disconnect");
            }

            ClearSuppressedBatchEvents();
            lock (_stateLock)
            {
                _connectionTransition = null;
                Volatile.Write(ref _state, (int)ConnectionState.Disconnected);
                Volatile.Write(ref _stateDetail, null);
                _disconnecting = false;
            }
            if (previous != ConnectionState.Disconnected)
                QueueStateChanged(ConnectionState.Disconnected, null);
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
        lock (_stateLock)
        {
            _disconnecting = true;
            _transitionVersion++;
            _connectionTransition?.TrySetException(new ObjectDisposedException(nameof(TikTokLiveClient)));
            _connectionTransition = null;
        }

        await _lifecycle.WaitAsync().ConfigureAwait(false);
        try
        {
            if (!_native.IsClosed && !_native.IsInvalid)
            {
                try
                {
                    await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_disconnect),
                        CancellationToken.None).ConfigureAwait(false);
                }
                catch { /* SafeHandle release below performs the final synchronous shutdown. */ }
            }
            _native.Dispose();
            ClearSuppressedBatchEvents();
        }
        finally
        {
            lock (_stateLock)
            {
                Volatile.Write(ref _state, (int)ConnectionState.Disconnected);
                Volatile.Write(ref _stateDetail, null);
            }
            _lifecycle.Release();
            _dispatch.Writer.TryComplete();
        }

        // Synchronous disposal from this client's own event handler must let the dispatcher unwind.
        if (!ReferenceEquals(DispatchOwner.Value, this)) await _dispatchTask.ConfigureAwait(false);
    }

    internal void OnNativeEvent(nint borrowed)
    {
        lock (_suppressionLock)
        {
            if (_suppressedBatchEvents.Remove(borrowed)) return;
        }
        if (Volatile.Read(ref _disposeRequested) != 0 || !HasIndividualSubscribers) return;

        long estimatedBytes = EstimateEventCallbackBytes(borrowed);
        if (!TryReserveQueueBytes(estimatedBytes))
        {
            RecordDroppedWork(1);
            return;
        }

        NativeEventHandle owned;
        try { owned = NativeEventHandle.Clone(borrowed); }
        catch
        {
            ReleaseQueueBytes(estimatedBytes);
            throw;
        }
        EnqueueReserved(new DispatchWork(() =>
        {
            using (owned)
            {
                if (Volatile.Read(ref _disposeRequested) != 0)
                {
                    RecordDroppedWork(1);
                    return;
                }
                try { PublishEvent(NativeEventReader.CaptureEvent(owned.DangerousGetHandle(), _options.CaptureFullFieldTree)); }
                catch (Exception ex)
                {
                    Interlocked.Increment(ref _processingFailures);
                    PublishError(new TikTokLiveException($"Could not copy native event: {ex.Message}", -1, inner: ex));
                }
            }
        }, owned.Dispose, 1, estimatedBytes));
    }

    internal void OnNativeBatch(nint borrowed)
    {
        if (Volatile.Read(ref _disposeRequested) != 0 || (!HasIndividualSubscribers && BatchReceived is null)) return;

        nuint nativeCount = NativeMethods.ttl_batch_event_count(borrowed);
        if (nativeCount > (nuint)_options.MaxBatchEvents)
        {
            if (!HasIndividualSubscribers)
                Interlocked.Add(ref _droppedEvents, checked((long)nativeCount));
            Interlocked.Increment(ref _droppedWorkItems);
            QueueError(new TikTokLiveException(
                $"Native batch of {nativeCount} events exceeds the configured limit of {_options.MaxBatchEvents}; individual event callbacks remain enabled.", -1));
            return;
        }

        var copiedEvents = new List<NativeEventHandle>((int)nativeCount);
        var borrowedPointers = new List<nint>((int)nativeCount);
        long estimatedBytes = 0;
        bool queueBytesReserved = false;
        try
        {
            for (nuint i = 0; i < nativeCount; i++)
            {
                nint item = NativeMethods.ttl_batch_event_at(borrowed, i);
                if (item == 0) throw new InvalidDataException($"Native batch event {i} is null.");
                borrowedPointers.Add(item);
                estimatedBytes = checked(estimatedBytes + EstimateEventCallbackBytes(item));
            }
            if (!TryReserveQueueBytes(estimatedBytes))
            {
                if (!HasIndividualSubscribers) Interlocked.Add(ref _droppedEvents, checked((long)nativeCount));
                Interlocked.Increment(ref _droppedWorkItems);
                return;
            }
            queueBytesReserved = true;
            foreach (nint item in borrowedPointers) copiedEvents.Add(NativeEventHandle.Clone(item));
            var cursor = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_cursor(borrowed)) ?? string.Empty;
            var ext = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_internal_ext(borrowed)) ?? string.Empty;
            var server = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_push_server(borrowed)) ?? string.Empty;
            bool needAck = NativeMethods.ttl_batch_need_ack(borrowed);
            long heartbeat = NativeMethods.ttl_batch_heartbeat_duration(borrowed);
            NativeEventHandle[] owned = copiedEvents.ToArray();
            copiedEvents.Clear();

            // The native contract invokes the event callback for each batch member immediately after
            // this callback. Decode each event once and publish the batch and individual views together.
            lock (_suppressionLock)
                foreach (nint pointer in borrowedPointers) _suppressedBatchEvents.Add(pointer);

            EnqueueReserved(new DispatchWork(() =>
            {
                try
                {
                    if (Volatile.Read(ref _disposeRequested) != 0)
                    {
                        RecordDroppedWork(owned.Length);
                        return;
                    }
                    var events = new TtlEvent[owned.Length];
                    for (int i = 0; i < owned.Length; i++)
                        events[i] = NativeEventReader.CaptureEvent(owned[i].DangerousGetHandle(), _options.CaptureFullFieldTree);
                    var batch = new TtlEventBatch(Array.AsReadOnly(events), cursor, ext, server, needAck, heartbeat);
                    PublishBatch(batch);
                    foreach (TtlEvent value in events) PublishEvent(value);
                }
                catch (Exception ex)
                {
                    Interlocked.Increment(ref _processingFailures);
                    QueueError(new TikTokLiveException($"Could not copy native event batch: {ex.Message}", -1, inner: ex));
                }
                finally { foreach (NativeEventHandle handle in owned) handle.Dispose(); }
            }, () => { foreach (NativeEventHandle handle in owned) handle.Dispose(); }, owned.Length, estimatedBytes));
            queueBytesReserved = false;
        }
        catch (Exception ex)
        {
            Interlocked.Increment(ref _processingFailures);
            foreach (NativeEventHandle handle in copiedEvents) handle.Dispose();
            if (queueBytesReserved) ReleaseQueueBytes(estimatedBytes);
            QueueError(new TikTokLiveException($"Could not copy native event batch: {ex.Message}", -1, inner: ex));
        }
    }

    internal void OnNativeState(int nativeState, string? detail)
    {
        if (!Enum.IsDefined(typeof(ConnectionState), nativeState) || Volatile.Read(ref _disposeRequested) != 0) return;
        var state = (ConnectionState)nativeState;
        long timeoutVersion = 0;
        Task? timeoutWait = null;
        lock (_stateLock)
        {
            if (_disconnecting || Volatile.Read(ref _disposeRequested) != 0) return;
            if (state == ConnectionState.Reconnecting &&
                (_connectionTransition is null || _connectionTransition.Task.IsCompleted))
            {
                _connectionTransition = NewTransition();
                timeoutVersion = ++_transitionVersion;
                timeoutWait = _connectionTransition.Task;
            }
            else if (state == ConnectionState.Connecting &&
                     (_connectionTransition is null || _connectionTransition.Task.IsCompleted))
            {
                _connectionTransition = NewTransition();
                timeoutVersion = ++_transitionVersion;
                timeoutWait = _connectionTransition.Task;
            }

            Volatile.Write(ref _state, nativeState);
            Volatile.Write(ref _stateDetail, detail);

            if (state == ConnectionState.Connected)
                _connectionTransition?.TrySetResult();
            else if (state is ConnectionState.Offline or ConnectionState.Error)
                _connectionTransition?.TrySetException(new TikTokLiveException(
                    detail ?? $"Connection entered {state}.", nativeState));
            else if (state == ConnectionState.Disconnected && _connectionTransition is { Task.IsCompleted: false })
                _connectionTransition.TrySetException(new TikTokLiveException(
                    detail ?? "Connection ended before it reached CONNECTED.", nativeState));
        }

        if (timeoutWait is not null) StartTransitionTimeout(timeoutVersion, timeoutWait);
        QueueStateChanged(state, detail);
    }

    internal void OnNativeError(TikTokLiveException error)
    {
        if (Volatile.Read(ref _disposeRequested) != 0) return;
        lock (_stateLock)
        {
            if (_disconnecting) return;
            if (error.Retryable != true)
                _connectionTransition?.TrySetException(error);
        }
        QueueError(error);
    }

    internal void OnInteropFailure(Exception error) => OnNativeError(
        new TikTokLiveException($"Native callback data could not be copied: {error.Message}", -1, inner: error));

    private bool HasIndividualSubscribers => EventReceived is not null || ChatReceived is not null ||
        GiftReceived is not null || LikeReceived is not null || MemberReceived is not null ||
        SocialReceived is not null || RoomUserReceived is not null;

    private void PublishEvent(TtlEvent value)
    {
        InvokeHandlers(EventReceived, new TikTokEventArgs(value));
        if (Volatile.Read(ref _disposeRequested) != 0) return;
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

    private void QueueStateChanged(ConnectionState state, string? detail) => Queue(new DispatchWork(() =>
    {
        if (Volatile.Read(ref _disposeRequested) == 0)
            InvokeHandlers(StateChanged, new ConnectionStateChangedEventArgs(state, detail));
    }));

    private void QueueError(TikTokLiveException error) => Queue(new DispatchWork(() =>
    {
        if (Volatile.Read(ref _disposeRequested) == 0) PublishError(error);
    }));

    private static async Task DispatchLoopAsync(ChannelReader<DispatchWork> reader,
        WeakReference<TikTokLiveClient> owner)
    {
        while (await reader.WaitToReadAsync().ConfigureAwait(false))
        {
            while (reader.TryRead(out DispatchWork? work))
            {
                owner.TryGetTarget(out TikTokLiveClient? client);
                DispatchOwner.Value = client;
                try { work.Execute(); }
                catch (Exception ex)
                {
                    if (client is not null)
                    {
                        Interlocked.Increment(ref client._processingFailures);
                        client.NotifyHandlerException(ex);
                    }
                }
                finally
                {
                    if (client is not null) client.ReleaseDispatchBytes(work);
                    DispatchOwner.Value = null;
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
            if (Volatile.Read(ref _disposeRequested) != 0) return;
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

    private bool Queue(DispatchWork work)
    {
        if (Volatile.Read(ref _disposeRequested) != 0)
        {
            DropDispatchedWork(work);
            return false;
        }
        if (_dispatch.Writer.TryWrite(work)) return true;
        DropDispatchedWork(work);
        return false;
    }

    private bool EnqueueReserved(DispatchWork work)
    {
        work.MarkAccounted();
        if (Volatile.Read(ref _disposeRequested) != 0)
        {
            DropDispatchedWork(work);
            return false;
        }
        if (_dispatch.Writer.TryWrite(work)) return true;
        DropDispatchedWork(work);
        return false;
    }

    private void DropDispatchedWork(DispatchWork work)
    {
        ReleaseDispatchBytes(work);
        Interlocked.Increment(ref _droppedWorkItems);
        if (work.EventCount != 0) Interlocked.Add(ref _droppedEvents, work.EventCount);
        work.Drop();
    }

    private void ReleaseDispatchBytes(DispatchWork work)
    {
        if (work.ReleaseAccounting()) ReleaseQueueBytes(work.EstimatedBytes);
    }

    private bool TryReserveQueueBytes(long bytes)
    {
        if (bytes < 0 || bytes > _options.EventQueueByteLimit) return false;
        while (true)
        {
            long current = Interlocked.Read(ref _queuedBytes);
            if (current > _options.EventQueueByteLimit - bytes) return false;
            if (Interlocked.CompareExchange(ref _queuedBytes, current + bytes, current) == current) return true;
        }
    }

    private void ReleaseQueueBytes(long bytes)
    {
        if (bytes != 0) Interlocked.Add(ref _queuedBytes, -bytes);
    }

    private void RecordDroppedWork(int eventCount)
    {
        Interlocked.Increment(ref _droppedWorkItems);
        if (eventCount != 0) Interlocked.Add(ref _droppedEvents, eventCount);
    }

    private void ClearSuppressedBatchEvents()
    {
        lock (_suppressionLock) _suppressedBatchEvents.Clear();
    }

    private static long EstimateEventCallbackBytes(nint value)
    {
        nuint rawSize = NativeMethods.ttl_event_raw_size(value);
        if (rawSize > int.MaxValue) return long.MaxValue;
        // Include room for the native clone, raw payload, copied strings/fields, and managed objects.
        return checked((long)rawSize * 4 + 256 * 1024);
    }

    private int ReadQueueDepth()
    {
        try { return _dispatch.Reader.Count; }
        catch (NotSupportedException) { return 0; }
    }

    private void StartTransitionTimeout(long version, Task transition)
    {
        if (_options.ConnectTimeout == Timeout.InfiniteTimeSpan) return;
        _ = MonitorTransitionTimeoutAsync(version, transition);
    }

    private async Task MonitorTransitionTimeoutAsync(long version, Task transition)
    {
        try
        {
            long started = Stopwatch.GetTimestamp();
            TimeSpan maxDelay = TimeSpan.FromMilliseconds(int.MaxValue - 1);
            while (true)
            {
                TimeSpan remaining = _options.ConnectTimeout - Stopwatch.GetElapsedTime(started);
                if (remaining <= TimeSpan.Zero) break;
                Task delay = Task.Delay(remaining > maxDelay ? maxDelay : remaining);
                if (await Task.WhenAny(transition, delay).ConfigureAwait(false) != delay) return;
            }
            await TimeoutTransitionAsync(version).ConfigureAwait(false);
        }
        catch { /* Transition monitoring must never create an unobserved task failure. */ }
    }

    private async Task TimeoutTransitionAsync(long version)
    {
        await _lifecycle.WaitAsync().ConfigureAwait(false);
        try
        {
            TaskCompletionSource? transition;
            lock (_stateLock)
            {
                if (Volatile.Read(ref _disposeRequested) != 0 || _disconnecting ||
                    version != _transitionVersion || _connectionTransition is null ||
                    _connectionTransition.Task.IsCompleted)
                    return;
                transition = _connectionTransition;
                _disconnecting = true;
                _transitionVersion++;
            }

            int result;
            try
            {
                result = await Task.Run(() => InvokeClient(NativeMethods.ttl_live_client_disconnect),
                    CancellationToken.None).ConfigureAwait(false);
            }
            catch (Exception error)
            {
                string failedDetail = $"Connection attempt timed out and native shutdown failed: {error.Message}";
                lock (_stateLock)
                {
                    Volatile.Write(ref _state, (int)ConnectionState.Error);
                    Volatile.Write(ref _stateDetail, failedDetail);
                    _connectionTransition = null;
                    _disconnecting = false;
                    transition.TrySetException(new TimeoutException(failedDetail, error));
                }
                QueueStateChanged(ConnectionState.Error, failedDetail);
                return;
            }
            string? detail = result == 0 ? "Connection attempt timed out." :
                $"Connection attempt timed out and native shutdown failed ({NativeRuntime.LastError("timeout disconnect").Message}).";
            lock (_stateLock)
            {
                Volatile.Write(ref _state, (int)(result == 0 ? ConnectionState.Disconnected : ConnectionState.Error));
                Volatile.Write(ref _stateDetail, detail);
                _connectionTransition = null;
                _disconnecting = false;
                transition.TrySetException(new TimeoutException("The native connection attempt exceeded ConnectTimeout."));
            }
            if (result == 0) ClearSuppressedBatchEvents();
            QueueStateChanged(result == 0 ? ConnectionState.Disconnected : ConnectionState.Error, detail);
        }
        finally { _lifecycle.Release(); }
    }

    private void FailTransition(long version, Exception error, ConnectionState state, string? detail)
    {
        bool changed = false;
        lock (_stateLock)
        {
            if (version != _transitionVersion) return;
            _connectionTransition?.TrySetException(error);
            Volatile.Write(ref _state, (int)state);
            Volatile.Write(ref _stateDetail, detail);
            changed = true;
        }
        if (changed) QueueStateChanged(state, detail);
    }

    private static TaskCompletionSource NewTransition()
    {
        var transition = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _ = transition.Task.ContinueWith(static task => _ = task.Exception, CancellationToken.None,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
        return transition;
    }

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

    private sealed class DispatchWork(Action execute, Action? onDrop = null, int eventCount = 0,
        long estimatedBytes = 0)
    {
        private int _dropped;
        private int _accounted;
        internal int EventCount { get; } = eventCount;
        internal long EstimatedBytes { get; } = estimatedBytes;
        internal void Execute() => execute();
        internal void MarkAccounted() => Interlocked.Exchange(ref _accounted, 1);
        internal bool ReleaseAccounting() => Interlocked.Exchange(ref _accounted, 0) == 1;
        internal void Drop()
        {
            if (Interlocked.Exchange(ref _dropped, 1) == 0) onDrop?.Invoke();
        }
    }
}
