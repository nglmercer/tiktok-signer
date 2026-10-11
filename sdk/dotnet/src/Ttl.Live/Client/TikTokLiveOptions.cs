namespace Ttl.Live;

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

    internal void Validate()
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(UniqueId);
        if (MaxReconnectAttempts > 1000) throw new ArgumentOutOfRangeException(nameof(MaxReconnectAttempts), "The native ABI allows at most 1000 retries.");
        if (ConnectTimeout <= TimeSpan.Zero && ConnectTimeout != Timeout.InfiniteTimeSpan)
            throw new ArgumentOutOfRangeException(nameof(ConnectTimeout), "Connect timeout must be positive or infinite.");
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
