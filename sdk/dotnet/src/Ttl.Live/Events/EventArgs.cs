namespace Ttl.Live;

public sealed class TikTokEventArgs(TtlEvent @event) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public string Method => Event.Method;
    public uint MethodId => Event.MethodId;
    public uint EventType => Event.EventType;
    public ulong MessageId => Event.MessageId;
    public bool IsHistory => Event.IsHistory;
    public EventSupport Support => Event.Support;
    public string? SchemaName => Event.SchemaName;
    public ulong? Timestamp => Event.Timestamp;
    public ReadOnlyMemory<byte> RawPayload => Event.RawPayload;
    public TtlObject Root => Event.Root;
    public NormalizedLiveEvent? Normalized => Event.Normalized;
    public string? ToJson(JsonMode mode) => Event.ToJson(mode);
}

public sealed class TikTokBatchEventArgs(TtlEventBatch batch) : EventArgs
{
    public TtlEventBatch Batch { get; } = batch;
}

public sealed class ChatReceivedEventArgs(TtlEvent @event, ChatLiveEvent chat) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public string Method => Event.Method;
    public ulong MessageId => Event.MessageId;
    public ulong? Timestamp => Event.Timestamp;
    public bool IsHistory => Event.IsHistory;
    public TtlUser User => chat.User;
    public string Comment => chat.Comment;
}

public sealed class GiftReceivedEventArgs(TtlEvent @event, GiftLiveEvent gift) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public string Method => Event.Method;
    public ulong MessageId => Event.MessageId;
    public ulong? Timestamp => Event.Timestamp;
    public bool IsHistory => Event.IsHistory;
    public TtlUser User => gift.User;
    public ulong GiftId => gift.GiftId;
    public string GiftName => gift.GiftName;
    public ulong DiamondCount => gift.DiamondCount;
    public ulong RepeatCount => gift.RepeatCount;
    public ulong ComboCount => gift.ComboCount;
    public ulong GroupId => gift.GroupId;
    public bool RepeatEnd => gift.RepeatEnd;
    public string? GiftImageUrl => gift.GiftImageUrl;
}

public sealed class LikeReceivedEventArgs(TtlEvent @event, LikeLiveEvent like) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public TtlUser User => like.User;
    public ulong Count => like.Count;
    public ulong Total => like.Total;
}

public sealed class MemberReceivedEventArgs(TtlEvent @event, MemberLiveEvent member) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public TtlUser User => member.User;
    public ulong MemberCount => member.MemberCount;
    public int Action => member.Action;
}

public sealed class SocialReceivedEventArgs(TtlEvent @event, SocialLiveEvent social) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public TtlUser User => social.User;
    public long Action => social.Action;
    public ulong FollowCount => social.FollowCount;
    public ulong ShareCount => social.ShareCount;
}

public sealed class RoomUserReceivedEventArgs(TtlEvent @event, RoomUserLiveEvent roomUser) : EventArgs
{
    public TtlEvent Event { get; } = @event;
    public ulong Total => roomUser.Total;
    public ulong Popularity => roomUser.Popularity;
    public ulong TotalUser => roomUser.TotalUser;
    public ulong Anonymous => roomUser.Anonymous;
    public IReadOnlyList<TopViewer> TopViewers => roomUser.TopViewers;
    public IReadOnlyList<TopViewer> RankedViewers => roomUser.RankedViewers;
}

public sealed class ConnectionStateChangedEventArgs(ConnectionState state, string? detail) : EventArgs
{
    public ConnectionState State { get; } = state;
    public string? Detail { get; } = detail;
}

public sealed class NativeErrorEventArgs(TikTokLiveException error) : EventArgs
{
    public TikTokLiveException Error { get; } = error;
}

public sealed class EventHandlerExceptionEventArgs(Exception exception) : EventArgs
{
    public Exception Exception { get; } = exception;
}
