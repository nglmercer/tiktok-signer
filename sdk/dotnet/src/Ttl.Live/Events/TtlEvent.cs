namespace Ttl.Live;

public sealed class TtlEvent
{
    private readonly IReadOnlyDictionary<JsonMode, string?> _json;
    private readonly byte[] _rawPayload;
    public string Method { get; }
    public uint MethodId { get; }
    public uint EventType { get; }
    public ulong MessageId { get; }
    public bool IsHistory { get; }
    public EventSupport Support { get; }
    public string? SchemaName { get; }
    public ulong? Timestamp { get; }
    public uint Flags { get; }
    public bool IsTruncated => (Flags & 1) != 0;
    public bool HasUnknownFields => (Flags & 4) != 0;
    public bool HasDecodeWarning => (Flags & 8) != 0;
    public ReadOnlyMemory<byte> RawPayload => _rawPayload;
    public TtlObject Root { get; }
    public NormalizedLiveEvent? Normalized { get; }

    internal TtlEvent(string method, uint methodId, uint eventType, ulong messageId,
        bool isHistory, EventSupport support, string? schemaName, ulong? timestamp,
        uint flags, byte[] rawPayload, TtlObject root, NormalizedLiveEvent? normalized,
        IReadOnlyDictionary<JsonMode, string?> json)
    {
        Method = method; MethodId = methodId; EventType = eventType; MessageId = messageId;
        IsHistory = isHistory; Support = support; SchemaName = schemaName;
        Timestamp = timestamp; Flags = flags; _rawPayload = rawPayload; Root = root;
        Normalized = normalized; _json = json;
    }

    /// <summary>Returns native serialization for a lossless/compact JSON mode; normalized JSON is null when unsupported.</summary>
    public string? ToJson(JsonMode mode) => _json.TryGetValue(mode, out string? value)
        ? value
        : throw new ArgumentOutOfRangeException(nameof(mode));

    /// <summary>Creates an independent managed copy; its payload and field tree are detached from the original event.</summary>
    public TtlEvent Clone() => new(Method, MethodId, EventType, MessageId, IsHistory,
        Support, SchemaName, Timestamp, Flags, _rawPayload.ToArray(), Root.Clone(),
        Normalized, new System.Collections.ObjectModel.ReadOnlyDictionary<JsonMode, string?>(
            new Dictionary<JsonMode, string?>(_json)));
}

public sealed record TtlEventBatch(
    IReadOnlyList<TtlEvent> Events,
    string Cursor,
    string InternalExt,
    string PushServer,
    bool NeedAck,
    long HeartbeatDuration);

public abstract record NormalizedLiveEvent;
public sealed record TtlUser(ulong Id, string Nickname, string UniqueId, string SecUid, string AvatarUrl);
public sealed record ChatLiveEvent(TtlUser User, string Comment) : NormalizedLiveEvent;
public sealed record GiftLiveEvent(TtlUser User, ulong GiftId, string GiftName, ulong DiamondCount,
    ulong RepeatCount, ulong ComboCount, ulong GroupId, bool RepeatEnd, string? GiftImageUrl) : NormalizedLiveEvent;
public sealed record LikeLiveEvent(TtlUser User, ulong Count, ulong Total) : NormalizedLiveEvent;
public sealed record MemberLiveEvent(TtlUser User, ulong MemberCount, int Action) : NormalizedLiveEvent;
public sealed record SocialLiveEvent(TtlUser User, long Action, ulong FollowCount, ulong ShareCount) : NormalizedLiveEvent;
public sealed record RoomUserLiveEvent(ulong Total, ulong Popularity, ulong TotalUser, ulong Anonymous,
    IReadOnlyList<TopViewer> TopViewers, IReadOnlyList<TopViewer> RankedViewers) : NormalizedLiveEvent;
public sealed record TopViewer(ulong Rank, ulong Score, ulong Delta, TtlUser User);
