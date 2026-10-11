using System.Runtime.InteropServices;
using System.Threading.Channels;

namespace Ttl.Live.Native;

internal sealed class CallbackContext(WeakReference<TikTokLiveClient> client,
    ChannelWriter<Action> dispatchWriter)
{
    internal WeakReference<TikTokLiveClient> Client { get; } = client;
    internal void CompleteDispatch() => dispatchWriter.TryComplete();
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

    private static bool TryClient(nint userdata, out TikTokLiveClient? client)
    {
        client = null;
        if (userdata == 0) return false;
        try
        {
            var handle = GCHandle.FromIntPtr(userdata);
            return handle.Target is CallbackContext context && context.Client.TryGetTarget(out client);
        }
        catch { return false; }
    }

    private static void OnEvent(nint userdata, nint value)
    {
        if (!TryClient(userdata, out var client)) return;
        try { client!.OnNativeEvent(value); }
        catch (Exception error) { client!.OnInteropFailure(error); }
    }

    private static void OnBatch(nint userdata, nint value)
    {
        if (!TryClient(userdata, out var client)) return;
        try { client!.OnNativeBatch(value); }
        catch (Exception error) { client!.OnInteropFailure(error); }
    }

    private static void OnState(nint userdata, int state, nint detail, nuint detailLength)
    {
        if (!TryClient(userdata, out var client)) return;
        try
        {
            string? copied = NativeRuntime.ReadOptionalUtf8(detail, detailLength);
            client!.OnNativeState(state, copied);
        }
        catch (Exception error) { client!.OnInteropFailure(error); }
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
        catch (Exception exception) { client!.OnInteropFailure(exception); }
    }
}
