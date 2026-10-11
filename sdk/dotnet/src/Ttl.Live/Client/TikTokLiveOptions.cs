namespace Ttl.Live;

public enum EventOverflowPolicy
{
    /// <summary>Keep queued work and reject new callback work until capacity is available.</summary>
    DropNewest = 0,
    /// <summary>Discard the oldest queued callback work to admit the newest work.</summary>
    DropOldest = 1
}

public sealed class TikTokLiveOptions
{
    public required string UniqueId { get; init; }
    /// <summary>Trusted JavaScript signing bundle source. When omitted, BundlePath or TTL_BUNDLE is used.</summary>
    public string? Bundle { get; init; }
    /// <summary>Path to a trusted JavaScript signing bundle source file.</summary>
    public string? BundlePath { get; init; }
    public string? CookieHeader { get; init; }
    public bool AutoReconnect { get; init; }
    public uint MaxReconnectAttempts { get; init; } = 10;
    public TimeSpan ConnectTimeout { get; init; } = TimeSpan.FromSeconds(60);
    /// <summary>Maximum number of pending managed callback work items per client.</summary>
    public int EventQueueCapacity { get; init; } = 256;
    /// <summary>Approximate total memory budget for queued and currently dispatched event work.</summary>
    public long EventQueueByteLimit { get; init; } = 64L * 1024 * 1024;
    /// <summary>Behavior when the managed callback queue is full. Dropped work is counted in diagnostics.</summary>
    public EventOverflowPolicy EventOverflowPolicy { get; init; } = EventOverflowPolicy.DropNewest;
    /// <summary>Maximum number of events copied from one native batch callback.</summary>
    public int MaxBatchEvents { get; init; } = 256;
    /// <summary>Copy the complete protobuf field tree. Disable only when normalized and raw data are sufficient.</summary>
    public bool CaptureFullFieldTree { get; init; } = true;

    internal void Validate()
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(UniqueId);
        if (MaxReconnectAttempts > 1000) throw new ArgumentOutOfRangeException(nameof(MaxReconnectAttempts), "The native ABI allows at most 1000 retries.");
        if (ConnectTimeout <= TimeSpan.Zero && ConnectTimeout != Timeout.InfiniteTimeSpan)
            throw new ArgumentOutOfRangeException(nameof(ConnectTimeout), "Connect timeout must be positive or infinite.");
        if (EventQueueCapacity is < 1 or > 65536)
            throw new ArgumentOutOfRangeException(nameof(EventQueueCapacity), "Event queue capacity must be between 1 and 65536.");
        if (EventQueueByteLimit is < 1_048_576 or > 1_073_741_824)
            throw new ArgumentOutOfRangeException(nameof(EventQueueByteLimit), "Event queue byte limit must be between 1 MiB and 1 GiB.");
        if (!Enum.IsDefined(EventOverflowPolicy))
            throw new ArgumentOutOfRangeException(nameof(EventOverflowPolicy));
        if (MaxBatchEvents is < 1 or > 8192)
            throw new ArgumentOutOfRangeException(nameof(MaxBatchEvents), "Maximum batch size must be between 1 and the native ABI bound of 8192.");
    }

    internal string ResolveBundleSource()
    {
        if (Bundle is not null && BundlePath is not null)
            throw new ArgumentException("Set either Bundle or BundlePath, not both.");
        if (Bundle is not null) return ValidateBundle(Bundle);
        string? path = BundlePath ?? Environment.GetEnvironmentVariable("TTL_BUNDLE");
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("ttl-live-native requires a trusted JavaScript signing bundle. Set Bundle, BundlePath, or TTL_BUNDLE to the bundle file path.");
        try { return ValidateBundle(File.ReadAllText(path)); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            throw new ArgumentException($"Unable to read the signing bundle at '{path}': {ex.Message}", nameof(BundlePath), ex);
        }
    }

    private static string ValidateBundle(string source)
    {
        if (string.IsNullOrWhiteSpace(source)) throw new ArgumentException("The signing bundle source cannot be empty.");
        if (source.Contains('\0')) throw new ArgumentException("The signing bundle source cannot contain NUL.");
        return source;
    }
}

public enum ConnectionState
{
    Disconnected = 0,
    Connecting = 1,
    Connected = 2,
    Reconnecting = 3,
    Offline = 4,
    Error = 5
}
