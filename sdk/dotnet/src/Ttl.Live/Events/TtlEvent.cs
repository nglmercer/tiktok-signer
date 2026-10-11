using System.Collections.Concurrent;
using System.Collections.ObjectModel;

namespace Ttl.Live;

public sealed class TtlEvent
{
    private readonly ConcurrentDictionary<JsonMode, Lazy<JsonResult>> _json = new();
    private readonly byte[] _rawPayload;
    private readonly Func<string, ulong, bool, byte[], JsonMode, string?> _jsonFactory;
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
        IReadOnlyDictionary<JsonMode, string?> json,
        Func<string, ulong, bool, byte[], JsonMode, string?> jsonFactory)
    {
        Method = method; MethodId = methodId; EventType = eventType; MessageId = messageId;
        IsHistory = isHistory; Support = support; SchemaName = schemaName;
        Timestamp = timestamp; Flags = flags; _rawPayload = rawPayload; Root = root;
        Normalized = normalized; _jsonFactory = jsonFactory;
        foreach (var pair in json)
            _json[pair.Key] = new Lazy<JsonResult>(() => new JsonResult(pair.Value),
                LazyThreadSafetyMode.ExecutionAndPublication);
    }

    /// <summary>Returns native serialization for the requested mode; serialization is deferred until first use.</summary>
    public string? ToJson(JsonMode mode)
    {
        if (!Enum.IsDefined(mode)) throw new ArgumentOutOfRangeException(nameof(mode));
        return _json.GetOrAdd(mode, selected => new Lazy<JsonResult>(
            () => new JsonResult(_jsonFactory(Method, MessageId, IsHistory, _rawPayload, selected)),
            LazyThreadSafetyMode.ExecutionAndPublication)).Value.Value;
    }

    /// <summary>Creates an independent managed copy; its payload and field tree are detached from the original event.</summary>
    public TtlEvent Clone()
    {
        var cached = new Dictionary<JsonMode, string?>();
        foreach (var pair in _json)
            if (pair.Value.IsValueCreated) cached[pair.Key] = pair.Value.Value.Value;
        return new TtlEvent(Method, MethodId, EventType, MessageId, IsHistory,
            Support, SchemaName, Timestamp, Flags, _rawPayload.ToArray(), Root.Clone(),
            CloneNormalized(Normalized), new ReadOnlyDictionary<JsonMode, string?>(cached), _jsonFactory);
    }

    private static NormalizedLiveEvent? CloneNormalized(NormalizedLiveEvent? value) => value switch
    {
        RoomUserLiveEvent room => room with
        {
            TopViewers = Array.AsReadOnly(room.TopViewers.ToArray()),
            RankedViewers = Array.AsReadOnly(room.RankedViewers.ToArray())
        },
        _ => value
    };

    private sealed record JsonResult(string? Value);
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
