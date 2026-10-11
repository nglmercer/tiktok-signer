using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Threading.Channels;
using Ttl.Live;

/// <summary>Fan-out event feed with independent bounded buffers and a finite VRChat snapshot.</summary>
public sealed class LiveEventFeed : IDisposable
{
    private static readonly JsonSerializerOptions SnapshotJsonOptions = new()
    {
        Encoder = JavaScriptEncoder.Default,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase
    };

    private readonly object _gate = new();
    private readonly Dictionary<long, Channel<RelayEvent>> _subscribers = [];
    private readonly Queue<SnapshotEntry> _history = new();
    private readonly int _subscriberQueueCapacity;
    private readonly int _historyCapacity;
    private readonly int _maxPayloadBytes;
    private readonly HashSet<string> _allowedIps;
    private long _sequence;
    private long _nextSubscriberId;
    private long _droppedSubscriberEvents;
    private long _droppedSnapshotEvents;
    private bool _disposed;

    public LiveEventFeed(int subscriberQueueCapacity = 64, int historyCapacity = 256,
        int maxPayloadBytes = 65_536, IEnumerable<string>? allowedIps = null)
    {
        if (subscriberQueueCapacity < 1) throw new ArgumentOutOfRangeException(nameof(subscriberQueueCapacity));
        if (historyCapacity < 1) throw new ArgumentOutOfRangeException(nameof(historyCapacity));
        if (maxPayloadBytes < 128) throw new ArgumentOutOfRangeException(nameof(maxPayloadBytes));
        _subscriberQueueCapacity = subscriberQueueCapacity;
        _historyCapacity = historyCapacity;
        _maxPayloadBytes = maxPayloadBytes;
        _allowedIps = new HashSet<string>(allowedIps ?? [], StringComparer.OrdinalIgnoreCase);
    }

    public static LiveEventFeed FromEnvironment()
    {
        int subscriberCapacity = ReadBoundedInt("LIVE_SSE_QUEUE_CAPACITY", 64, 1, 4096);
        int historyCapacity = ReadBoundedInt("VRCHAT_HISTORY_EVENTS", 256, 1, 100_000);
        int maxPayloadBytes = ReadBoundedInt("VRCHAT_MAX_PAYLOAD_BYTES", 65_536, 128, 4 * 1024 * 1024);
        string[] allowedIps = (Environment.GetEnvironmentVariable("VRCHAT_ALLOWED_IPS") ?? string.Empty)
            .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        return new LiveEventFeed(subscriberCapacity, historyCapacity, maxPayloadBytes, allowedIps);
    }

    public int SubscriberCount { get { lock (_gate) return _subscribers.Count; } }
    public long DroppedSubscriberEvents => Interlocked.Read(ref _droppedSubscriberEvents);
    public long DroppedSnapshotEvents => Interlocked.Read(ref _droppedSnapshotEvents);

    /// <summary>Publishes once to every current subscriber and stores only normalized chat/gift history.</summary>
    public long Publish(TtlEvent item)
    {
        ArgumentNullException.ThrowIfNull(item);
        lock (_gate)
        {
            if (_disposed) return _sequence;
            long sequence = ++_sequence;
            var published = new RelayEvent(sequence, item);
            foreach (Channel<RelayEvent> subscriber in _subscribers.Values)
                subscriber.Writer.TryWrite(published);

            SnapshotEntry? entry = CreateSnapshotEntry(sequence, item.Normalized);
            if (entry is not null)
            {
                _history.Enqueue(entry);
                while (_history.Count > _historyCapacity)
                {
                    _history.Dequeue();
                    Interlocked.Increment(ref _droppedSnapshotEvents);
                }
                TrimHistoryToPayloadLimitLocked();
            }
            return sequence;
        }
    }

    /// <summary>Gets a bounded, consistent chat/gift snapshot suitable for fixed-URL polling.</summary>
    public byte[] GetSnapshotPayload()
    {
        lock (_gate) return SerializeSnapshotLocked();
    }

    public bool IsIpAllowed(System.Net.IPAddress? address)
    {
        if (_allowedIps.Count == 0) return true;
        if (address is null) return false;
        string value = address.IsIPv4MappedToIPv6 ? address.MapToIPv4().ToString() : address.ToString();
        return _allowedIps.Contains(value);
    }

    public async IAsyncEnumerable<RelayEvent> Subscribe(
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        long id = 0;
        Channel<RelayEvent>? channel = null;
        lock (_gate)
        {
            if (!_disposed)
            {
                id = ++_nextSubscriberId;
                channel = Channel.CreateBounded<RelayEvent>(new BoundedChannelOptions(_subscriberQueueCapacity)
                {
                    SingleReader = true,
                    SingleWriter = false,
                    AllowSynchronousContinuations = false,
                    FullMode = BoundedChannelFullMode.DropOldest
                }, _ => Interlocked.Increment(ref _droppedSubscriberEvents));
                _subscribers.Add(id, channel);
            }
        }
        if (channel is null) yield break;

        try
        {
            await foreach (RelayEvent item in channel.Reader.ReadAllAsync(cancellationToken).ConfigureAwait(false))
                yield return item;
        }
        finally
        {
            lock (_gate)
            {
                _subscribers.Remove(id);
                channel.Writer.TryComplete();
            }
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
            foreach (Channel<RelayEvent> subscriber in _subscribers.Values) subscriber.Writer.TryComplete();
            _subscribers.Clear();
        }
    }

    private byte[] SerializeSnapshotLocked()
    {
        while (true)
        {
            byte[] payload = JsonSerializer.SerializeToUtf8Bytes(CreateSnapshotLocked(), SnapshotJsonOptions);
            if (payload.Length <= _maxPayloadBytes || _history.Count == 0) return payload;
            _history.Dequeue();
            Interlocked.Increment(ref _droppedSnapshotEvents);
        }
    }

    private VrChatEventsSnapshot CreateSnapshotLocked() => new(_sequence,
        Array.AsReadOnly(_history.Where(entry => entry.Chat is not null).Select(entry => entry.Chat!).ToArray()),
        Array.AsReadOnly(_history.Where(entry => entry.Gift is not null).Select(entry => entry.Gift!).ToArray()));

    private void TrimHistoryToPayloadLimitLocked()
    {
        while (_history.Count > 0 && EstimateSnapshotPayloadSizeLocked() > _maxPayloadBytes)
        {
            _history.Dequeue();
            Interlocked.Increment(ref _droppedSnapshotEvents);
        }
    }

    private int EstimateSnapshotPayloadSizeLocked()
    {
        var empty = new VrChatEventsSnapshot(_sequence, Array.Empty<VrChatChat>(), Array.Empty<VrChatGift>());
        int size = JsonSerializer.SerializeToUtf8Bytes(empty, SnapshotJsonOptions).Length;
        int chats = 0;
        int gifts = 0;
        foreach (SnapshotEntry entry in _history)
        {
            size += entry.SerializedLength;
            if (entry.Chat is not null) chats++;
            if (entry.Gift is not null) gifts++;
        }
        return size + Math.Max(0, chats - 1) + Math.Max(0, gifts - 1);
    }

    private static SnapshotEntry? CreateSnapshotEntry(long sequence, NormalizedLiveEvent? normalized) => normalized switch
    {
        ChatLiveEvent chat => CreateChatEntry(sequence, chat),
        GiftLiveEvent gift => CreateGiftEntry(sequence, gift),
        _ => null
    };

    private static SnapshotEntry CreateChatEntry(long sequence, ChatLiveEvent chat)
    {
        var value = new VrChatChat(sequence.ToString(System.Globalization.CultureInfo.InvariantCulture), UserName(chat.User), chat.Comment);
        return new SnapshotEntry(sequence, value, null,
            JsonSerializer.SerializeToUtf8Bytes(value, SnapshotJsonOptions).Length);
    }

    private static SnapshotEntry CreateGiftEntry(long sequence, GiftLiveEvent gift)
    {
        var value = new VrChatGift(sequence.ToString(System.Globalization.CultureInfo.InvariantCulture),
                gift.GiftId.ToString(System.Globalization.CultureInfo.InvariantCulture), gift.GiftName,
                UserName(gift.User), gift.RepeatCount > 0 ? gift.RepeatCount : gift.ComboCount > 0 ? gift.ComboCount : 1,
                gift.RepeatCount, gift.ComboCount, gift.GroupId.ToString(System.Globalization.CultureInfo.InvariantCulture),
                gift.RepeatEnd, gift.DiamondCount);
        return new SnapshotEntry(sequence, null, value,
            JsonSerializer.SerializeToUtf8Bytes(value, SnapshotJsonOptions).Length);
    }

    private static string UserName(TtlUser user) =>
        string.IsNullOrWhiteSpace(user.UniqueId) ? user.Nickname : user.UniqueId;

    private static int ReadBoundedInt(string name, int fallback, int minimum, int maximum) =>
        int.TryParse(Environment.GetEnvironmentVariable(name), out int value)
            ? Math.Clamp(value, minimum, maximum)
            : fallback;

    private sealed record SnapshotEntry(long Sequence, VrChatChat? Chat, VrChatGift? Gift, int SerializedLength);
}

public sealed record RelayEvent(long Sequence, TtlEvent Event);
public sealed record VrChatEventsSnapshot(long Sequence, IReadOnlyList<VrChatChat> Chat, IReadOnlyList<VrChatGift> Gifts);
public sealed record VrChatChat(string Id, string User, string Text);
public sealed record VrChatGift(string Id, string GiftId, string GiftName, string User,
    ulong Count, ulong RepeatCount, ulong ComboCount, string GroupId, bool RepeatEnd, ulong DiamondCount);
