using System.Text.RegularExpressions;
using Ttl.Live;

/// <summary>One operator-selected LIVE session. Credentials remain on the relay host.</summary>
public sealed class InteractiveLiveSession(LiveEventFeed feed, ILogger<InteractiveLiveSession> logger) : IAsyncDisposable
{
    private readonly SemaphoreSlim _operations = new(1, 1);
    private readonly object _gate = new();
    private TikTokLiveClient? _client;
    private Task _attempt = Task.CompletedTask;
    private LiveSessionStatus _status = new("", "Disconnected", false, "");
    private bool _disposed;
    private readonly string _streamId = Guid.NewGuid().ToString("N");

    public object Snapshot()
    {
        lock (_gate)
        {
            using var document = System.Text.Json.JsonDocument.Parse(feed.GetSnapshotPayload());
            return new { streamId = _streamId, connection = _status, events = document.RootElement.Clone() };
        }
    }

    public LiveSessionStatus Status { get { lock (_gate) return _status; } }

    public static string NormalizeCreator(string? input)
    {
        string creator = (input ?? "").Trim().TrimStart('@');
        if (!Regex.IsMatch(creator, @"\A[a-zA-Z0-9_.]{1,24}\z", RegexOptions.CultureInvariant))
            throw new ArgumentException("Enter a TikTok username (1–24 letters, digits, underscores or periods), not a URL.");
        return creator;
    }

    public async Task ConnectAsync(string? input)
    {
        string creator = NormalizeCreator(input);
        await _operations.WaitAsync();
        try
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            await StopLockedAsync();
            // Resolve the trusted bundle before accepting a connection; never accept source/cookies from HTTP.
            var client = new TikTokLiveClient(new TikTokLiveOptions
            {
                UniqueId = creator,
                AutoReconnect = true,
                CaptureFullFieldTree = false,
                CookieHeader = ResolveCookieHeader()
            });
            lock (_gate)
            {
                feed.ClearHistory();
                _client = client;
                _status = new(creator, "Connecting", false, "");
            }
            client.EventReceived += (_, args) =>
            {
                lock (_gate) { if (ReferenceEquals(_client, client)) feed.Publish(args.Event); }
            };
            client.StateChanged += (_, args) =>
            {
                lock (_gate)
                    if (ReferenceEquals(_client, client))
                        _status = _status with { State = args.State.ToString(), Error = args.State == ConnectionState.Connected ? "" : _status.Error };
            };
            client.ErrorReceived += (_, args) =>
            {
                logger.LogWarning("LIVE error for {Creator}, code {Code}", creator, args.Error.NativeCode);
                lock (_gate)
                    if (ReferenceEquals(_client, client))
                        _status = _status with { Error = $"LIVE error code {args.Error.NativeCode}; check server diagnostics." };
            };
            _attempt = ObserveConnectionAsync(client);
        }
        finally { _operations.Release(); }
    }

    private static string? ResolveCookieHeader()
    {
        string? cookie = Environment.GetEnvironmentVariable("TTL_COOKIE");
        if (!string.IsNullOrWhiteSpace(cookie)) return cookie;
        string? path = Environment.GetEnvironmentVariable("TTL_SESSION_FILE");
        return string.IsNullOrWhiteSpace(path) ? null : File.ReadAllText(path).Trim();
    }

    private async Task ObserveConnectionAsync(TikTokLiveClient client)
    {
        try { await client.ConnectAsync(); }
        catch (OperationCanceledException) { }
        catch (Exception error)
        {
            logger.LogWarning("LIVE connection failed ({Type})", error.GetType().Name);
            lock (_gate)
                if (ReferenceEquals(_client, client))
                    _status = _status with { State = client.State.ToString(), Error = "Connection failed; check server configuration and creator LIVE status." };
        }
    }

    public async Task DisconnectAsync()
    {
        await _operations.WaitAsync();
        try { await StopLockedAsync(); }
        finally { _operations.Release(); }
    }

    private async Task StopLockedAsync()
    {
        TikTokLiveClient? old;
        lock (_gate)
        {
            old = _client; _client = null;
            feed.ClearHistory();
            _status = new("", "Disconnected", false, "");
        }
        if (old is not null) await old.DisposeAsync();
        await _attempt;
    }

    public async Task LoadFixturesAsync(string fixtureDirectory)
    {
        // Decode through the real native SDK; fixtures never masquerade as a live connection.
        TtlEvent chat = TikTokEventDecoder.Decode("WebcastChatMessage", 1, false,
            await File.ReadAllBytesAsync(Path.Combine(fixtureDirectory, "chat.pb")));
        TtlEvent gift = TikTokEventDecoder.Decode("WebcastGiftMessage", 2, false,
            await File.ReadAllBytesAsync(Path.Combine(fixtureDirectory, "gift.pb")));
        await _operations.WaitAsync();
        try
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            await StopLockedAsync();
            lock (_gate)
            {
                feed.ClearHistory();
                _status = new("fixture", "Fixture", true, "Offline fixture data — not a LIVE connection.");
                feed.Publish(chat); feed.Publish(gift);
            }
        }
        finally { _operations.Release(); }
    }

    public async ValueTask DisposeAsync()
    {
        await _operations.WaitAsync();
        try
        {
            if (_disposed) return;
            _disposed = true;
            await StopLockedAsync();
        }
        finally { _operations.Release(); }
    }
}

public sealed record LiveSessionStatus(string Creator, string State, bool Fixture, string Error);
