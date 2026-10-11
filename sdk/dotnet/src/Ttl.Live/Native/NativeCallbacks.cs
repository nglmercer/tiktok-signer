using System.Runtime.InteropServices;

namespace Ttl.Live.Native;

internal sealed class CallbackContext(WeakReference<TikTokLiveClient> client)
{
    internal WeakReference<TikTokLiveClient> Client { get; } = client;
    private int _closed;
    internal bool TryGetClient(out TikTokLiveClient? value)
    {
        value = null;
        return Volatile.Read(ref _closed) == 0 && Client.TryGetTarget(out value);
    }
    internal void CompleteDispatch()
    {
        Interlocked.Exchange(ref _closed, 1);
    }
}

internal static class NativeCallbacks
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void EventDelegate(nint userdata, nint value);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void BatchDelegate(nint userdata, nint value);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void StateDelegate(nint userdata, int state, nint detail, nuint detailLength);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void ErrorDelegate(nint userdata, nint error);

    // Static roots keep every unmanaged entrypoint delegate alive until process exit.
    private static readonly EventDelegate EventRoot = OnEvent;
    private static readonly BatchDelegate BatchRoot = OnBatch;
    private static readonly StateDelegate StateRoot = OnState;
    private static readonly ErrorDelegate ErrorRoot = OnError;
    internal static nint EventPointer { get; } = Marshal.GetFunctionPointerForDelegate(EventRoot);
    internal static nint BatchPointer { get; } = Marshal.GetFunctionPointerForDelegate(BatchRoot);
    internal static nint StatePointer { get; } = Marshal.GetFunctionPointerForDelegate(StateRoot);
    internal static nint ErrorPointer { get; } = Marshal.GetFunctionPointerForDelegate(ErrorRoot);

    // Drives the same rooted unmanaged callback entrypoint used by Rust, without credentials or a
    // network. The native callback's pointer/length lifetime is identical to the C ABI contract.
    internal static void InvokeStateCallbackForTests(nint userdata, int state, string? detail)
    {
        using var utf8 = new Utf8Input(detail);
        StateRoot(userdata, state, utf8.Pointer, utf8.Length);
    }

    internal static void InvokeErrorCallbackForTests(nint userdata, int code, string detail, bool retryable)
    {
        using var utf8 = new Utf8Input(detail);
        nint pointer = Marshal.AllocHGlobal(Marshal.SizeOf<NativeErrorInfo>());
        try
        {
            Marshal.StructureToPtr(new NativeErrorInfo
            {
                Code = code,
                Message = utf8.Pointer,
                MessageLen = utf8.Length,
                Retryable = retryable ? (byte)1 : (byte)0,
                RetryAfterMs = -1,
                HttpStatus = 0
            }, pointer, false);
            ErrorRoot(userdata, pointer);
        }
        finally { Marshal.FreeHGlobal(pointer); }
    }

    private static bool TryClient(nint userdata, out TikTokLiveClient? client)
    {
        client = null;
        if (userdata == 0) return false;
        try
        {
            var handle = GCHandle.FromIntPtr(userdata);
            return handle.Target is CallbackContext context && context.TryGetClient(out client);
        }
        catch { return false; }
    }

    private static void OnEvent(nint userdata, nint value)
    {
        if (!TryClient(userdata, out var client)) return;
        try { client!.OnNativeEvent(value); }
        catch (Exception error) { try { client!.OnInteropFailure(error); } catch { } }
    }

    private static void OnBatch(nint userdata, nint value)
    {
        if (!TryClient(userdata, out var client)) return;
        try { client!.OnNativeBatch(value); }
        catch (Exception error) { try { client!.OnInteropFailure(error); } catch { } }
    }

    private static void OnState(nint userdata, int state, nint detail, nuint detailLength)
    {
        if (!TryClient(userdata, out var client)) return;
        try
        {
            string? copied = NativeRuntime.ReadOptionalUtf8(detail, detailLength);
            client!.OnNativeState(state, copied);
        }
        catch (Exception error) { try { client!.OnInteropFailure(error); } catch { } }
    }

    private static void OnError(nint userdata, nint error)
    {
        if (!TryClient(userdata, out var client)) return;
        try
        {
            if (error == 0) return;
            NativeErrorInfo info = Marshal.PtrToStructure<NativeErrorInfo>(error);
            TikTokLiveException copied = NativeRuntime.Error(info);
            client!.OnNativeError(copied);
        }
        catch (Exception exception) { try { client!.OnInteropFailure(exception); } catch { } }
    }
}
