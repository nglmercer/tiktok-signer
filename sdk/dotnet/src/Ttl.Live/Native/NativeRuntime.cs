using System.Runtime.InteropServices;
using System.Text;

namespace Ttl.Live.Native;

internal static class NativeRuntime
{
    private const uint ExpectedAbiVersion = 1;
    private static readonly Lazy<NativeLibraryInfo> Info = new(LoadAndCheck,
        LazyThreadSafetyMode.ExecutionAndPublication);

    internal static NativeLibraryInfo EnsureLoaded() => Info.Value;

    private static NativeLibraryInfo LoadAndCheck()
    {
        try
        {
            uint abi = NativeMethods.ttl_live_abi_version();
            string version = ReadNullTerminated(NativeMethods.ttl_live_version()) ?? "unknown";
            if (abi != ExpectedAbiVersion)
                throw new NativeLibraryException($"ttl-live-native ABI mismatch: SDK expects {ExpectedAbiVersion}, library reports {abi} (version {version}). Install a matching Ttl.Live native package.");
            return new NativeLibraryInfo(abi, version);
        }
        catch (NativeLibraryException) { throw; }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException or BadImageFormatException)
        {
            throw new NativeLibraryException(
                $"Unable to load a compatible ttl-live-native library for {RuntimeInformation.RuntimeIdentifier}. Install the matching Ttl.Live package runtime asset or set TTL_LIVE_NATIVE_LIBRARY. {ex.Message}", ex);
        }
    }

    internal static TikTokLiveException LastError(string operation)
    {
        int code;
        string message;
        try
        {
            code = NativeMethods.ttl_last_error_code();
            message = ReadNullTerminated(NativeMethods.ttl_last_error()) ?? "native operation failed without a message";
        }
        catch (Exception ex)
        {
            return new TikTokLiveException($"Native {operation} failed: {ex.Message}", -1, inner: ex);
        }
        return new TikTokLiveException($"Native {operation} failed ({(NativeResultCode)code}): {message}", code);
    }

    internal static TikTokLiveException Error(in NativeErrorInfo info)
    {
        string message = ReadUtf8(info.Message, info.MessageLen);
        return new TikTokLiveException(message, info.Code, info.Retryable != 0,
            info.RetryAfterMs < 0 ? null : info.RetryAfterMs,
            info.HttpStatus == 0 ? null : info.HttpStatus);
    }

    internal static string? ReadNullTerminated(nint value) =>
        value == 0 ? null : Marshal.PtrToStringUTF8(value);

    internal static string ReadUtf8(nint value, nuint length)
    {
        byte[] bytes = CopyBytes(value, length);
        return Encoding.UTF8.GetString(bytes);
    }

    internal static byte[] CopyBytes(nint value, nuint length)
    {
        if (length == 0) return Array.Empty<byte>();
        if (value == 0) throw new InvalidDataException("Native library returned a null pointer with a non-zero length.");
        if (length > int.MaxValue) throw new InvalidDataException($"Native buffer is too large for a managed array ({length} bytes).");
        var result = new byte[(int)length];
        Marshal.Copy(value, result, 0, result.Length);
        return result;
    }

    internal static string? ReadOptionalUtf8(nint value, nuint length) =>
        value == 0 ? null : ReadUtf8(value, length);
}

internal sealed class Utf8Input : IDisposable
{
    private nint _pointer;
    internal nuint Length { get; }
    internal nint Pointer => _pointer;

    internal Utf8Input(string? value)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(value ?? string.Empty);
        Length = (nuint)bytes.Length;
        if (bytes.Length == 0) return;
        _pointer = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, _pointer, bytes.Length);
    }

    public void Dispose()
    {
        nint old = Interlocked.Exchange(ref _pointer, 0);
        if (old != 0) Marshal.FreeHGlobal(old);
    }
}
