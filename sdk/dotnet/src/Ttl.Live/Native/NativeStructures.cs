using System.Runtime.InteropServices;

namespace Ttl.Live.Native;

[StructLayout(LayoutKind.Sequential)]
internal struct NativeLiveOptions
{
    internal uint StructSize;
    internal nint UniqueId;
    internal nuint UniqueIdLen;
    internal nint Bundle;
    internal nuint BundleLen;
    internal byte Reconnect;
    internal uint MaxReconnectAttempts;
    internal nint CookieHeader;
    internal nuint CookieHeaderLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeErrorInfo
{
    internal int Code;
    internal nint Message;
    internal nuint MessageLen;
    internal byte Retryable;
    internal long RetryAfterMs;
    internal int HttpStatus;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeEventInfo
{
    internal uint AbiVersion;
    internal nint Method;
    internal nuint MethodLen;
    internal ulong MessageId;
    internal int Support;
    internal byte IsHistory;
    internal nint SchemaName;
    internal nuint SchemaNameLen;
    internal nuint RawPayloadSize;
    internal uint MethodId;
    internal ulong Timestamp;
    internal byte HasTimestamp;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeUser
{
    internal ulong Id;
    internal nint Nickname;
    internal nuint NicknameLen;
    internal nint UniqueId;
    internal nuint UniqueIdLen;
    internal nint SecUid;
    internal nuint SecUidLen;
    internal nint AvatarUrl;
    internal nuint AvatarUrlLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeChatEvent
{
    internal NativeUser User;
    internal nint Comment;
    internal nuint CommentLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeGiftEvent
{
    internal NativeUser User;
    internal ulong GiftId;
    internal nint GiftName;
    internal nuint GiftNameLen;
    internal ulong DiamondCount;
    internal ulong RepeatCount;
    internal ulong ComboCount;
    internal ulong GroupId;
    internal byte RepeatEnd;
    internal nint GiftImageUrl;
    internal nuint GiftImageUrlLen;
}
