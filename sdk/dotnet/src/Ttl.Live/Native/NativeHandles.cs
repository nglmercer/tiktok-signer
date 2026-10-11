using Microsoft.Win32.SafeHandles;
using System.Runtime.InteropServices;

namespace Ttl.Live.Native;

internal sealed class NativeEventHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private NativeEventHandle() : base(true) { }
    internal NativeEventHandle(nint value) : base(true) => SetHandle(value);
    internal static NativeEventHandle Clone(nint borrowed)
    {
        nint clone = NativeMethods.ttl_event_clone(borrowed);
        if (clone == 0) throw NativeRuntime.LastError("clone event");
        return new NativeEventHandle(clone);
    }
    protected override bool ReleaseHandle()
    {
        try { NativeMethods.ttl_event_free(handle); return true; }
        catch { return false; }
    }
}

internal sealed class NativeBatchHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private NativeBatchHandle() : base(true) { }
    internal NativeBatchHandle(nint value) : base(true) => SetHandle(value);
    protected override bool ReleaseHandle()
    {
        try { NativeMethods.ttl_batch_free(handle); return true; }
        catch { return false; }
    }
}

internal sealed class NativeClientHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private GCHandle _callbackContext;

    internal NativeClientHandle(nint value, CallbackContext context) : base(true)
    {
        SetHandle(value);
        _callbackContext = GCHandle.Alloc(context, GCHandleType.Normal);
    }

    internal nint Userdata => GCHandle.ToIntPtr(_callbackContext);

    protected override bool ReleaseHandle()
    {
        bool callbacksCleared = false;
        bool destroyed = false;
        try
        {
            // The SDK serializes public operations and disconnects before Dispose. Retrying here
            // makes finalizer/constructor-failure paths safe too. Clearing each registration waits
            // for its in-flight callback before its userdata can be released.
            try { NativeMethods.ttl_live_client_disconnect(handle); } catch { }
            int eventResult = NativeMethods.ttl_live_client_set_event_callback(handle, 0, 0);
            int batchResult = NativeMethods.ttl_live_client_set_batch_callback(handle, 0, 0);
            int stateResult = NativeMethods.ttl_live_client_set_state_callback(handle, 0, 0);
            int errorResult = NativeMethods.ttl_live_client_set_error_callback(handle, 0, 0);
            callbacksCleared = eventResult == 0 && batchResult == 0 && stateResult == 0 && errorResult == 0;
            if (!callbacksCleared) return false;
            NativeMethods.ttl_live_client_destroy(handle);
            destroyed = true;
        }
        catch
        {
            // SafeHandle release runs on finalizer paths too; unmanaged exceptions
            // must not escape. Native destroy is synchronous and void-returning.
        }
        finally
        {
            if (callbacksCleared)
            {
                try
                {
                    if (_callbackContext.IsAllocated && _callbackContext.Target is CallbackContext context)
                        context.CompleteDispatch();
                }
                catch { }
                if (_callbackContext.IsAllocated) _callbackContext.Free();
                handle = 0;
            }
        }
        return destroyed && callbacksCleared;
    }
}
