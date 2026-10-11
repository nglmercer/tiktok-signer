namespace Ttl.Live;

public sealed record NativeLibraryInfo(uint AbiVersion, string Version);

public sealed class NativeLibraryException : Exception
{
    public NativeLibraryException(string message, Exception? inner = null) : base(message, inner) { }
}

public enum NativeResultCode
{
    Ok = 0, InvalidArgument = 1, InvalidUtf8 = 2, InvalidState = 3,
    Discovery = 4, Offline = 5, Signer = 6, Auth = 7, Socket = 8,
    Protocol = 9, Decode = 10, Timeout = 11, Shutdown = 12, Internal = 13
}

public sealed class TikTokLiveException : Exception
{
    public int NativeCode { get; }
    public bool? Retryable { get; }
    public long? RetryAfterMilliseconds { get; }
    public int? HttpStatus { get; }

    internal TikTokLiveException(string message, int nativeCode, bool? retryable = null,
        long? retryAfterMilliseconds = null, int? httpStatus = null, Exception? inner = null)
        : base(message, inner)
    {
        NativeCode = nativeCode;
        Retryable = retryable;
        RetryAfterMilliseconds = retryAfterMilliseconds;
        HttpStatus = httpStatus;
    }
}
