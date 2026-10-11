using System.Runtime.InteropServices;
using Ttl.Live.Native;

namespace Ttl.Live;

/// <summary>Offline decoder for fixture replay, archives, and schema inspection.</summary>
public static class TikTokEventDecoder
{
    public static TtlEvent Decode(string method, ulong messageId, bool isHistory, ReadOnlySpan<byte> payload)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(method);
        if (method.Contains('\0')) throw new ArgumentException("Event methods cannot contain NUL.", nameof(method));
        NativeRuntime.EnsureLoaded();
        using var nativeMethod = new Utf8Input(method);
        using var nativePayload = PinnedInput.From(payload);
        nint handle = NativeMethods.ttl_event_decode(nativeMethod.Pointer, nativeMethod.Length,
            messageId, isHistory, nativePayload.Pointer, nativePayload.Length);
        if (handle == 0) throw NativeRuntime.LastError("decode event");
        using var owned = new NativeEventHandle(handle);
        return NativeEventReader.CaptureEvent(handle);
    }

    public static TtlEventBatch DecodeBatch(ReadOnlySpan<byte> payload)
    {
        NativeRuntime.EnsureLoaded();
        using var nativePayload = PinnedInput.From(payload);
        nint handle = NativeMethods.ttl_batch_decode(nativePayload.Pointer, nativePayload.Length);
        if (handle == 0) throw NativeRuntime.LastError("decode batch");
        using var owned = new NativeBatchHandle(handle);
        return NativeEventReader.CaptureBatch(handle);
    }
}

internal sealed class PinnedInput : IDisposable
{
    private nint _pointer;
    internal nint Pointer => _pointer;
    internal nuint Length { get; }

    private PinnedInput(ReadOnlySpan<byte> payload)
    {
        Length = (nuint)payload.Length;
        if (payload.IsEmpty) return;
        _pointer = Marshal.AllocHGlobal(payload.Length);
        byte[] copy = payload.ToArray();
        Marshal.Copy(copy, 0, _pointer, copy.Length);
    }

    internal static PinnedInput From(ReadOnlySpan<byte> payload) => new(payload);

    public void Dispose()
    {
        nint pointer = Interlocked.Exchange(ref _pointer, 0);
        if (pointer != 0) Marshal.FreeHGlobal(pointer);
    }
}
