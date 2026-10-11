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
        bool destroyed = false;
        try
        {
            NativeMethods.ttl_live_client_set_event_callback(handle, 0, 0);
            NativeMethods.ttl_live_client_set_batch_callback(handle, 0, 0);
            NativeMethods.ttl_live_client_set_state_callback(handle, 0, 0);
            NativeMethods.ttl_live_client_set_error_callback(handle, 0, 0);
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
            try
            {
                if (_callbackContext.IsAllocated && _callbackContext.Target is CallbackContext context)
                    context.CompleteDispatch();
            }
            catch { }
            if (_callbackContext.IsAllocated) _callbackContext.Free();
            handle = 0;
        }
        return destroyed;
    }
}
