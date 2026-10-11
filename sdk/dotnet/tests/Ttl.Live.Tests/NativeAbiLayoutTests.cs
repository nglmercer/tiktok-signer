using System.Runtime.InteropServices;
using Ttl.Live.Native;

namespace Ttl.Live.Tests;

public sealed class NativeAbiLayoutTests
{
    [Fact]
    public void CStructsMatchThe64BitPublicHeaderLayouts()
    {
        Assert.Equal(8, IntPtr.Size);
        Assert.Equal(8, Marshal.SizeOf<nuint>());
        Assert.Equal(64, Marshal.SizeOf<NativeLiveOptions>());
        Assert.Equal(0, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.StructSize)).ToInt32());
        Assert.Equal(8, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.UniqueId)).ToInt32());
        Assert.Equal(16, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.UniqueIdLen)).ToInt32());
        Assert.Equal(24, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.Bundle)).ToInt32());
        Assert.Equal(32, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.BundleLen)).ToInt32());
        Assert.Equal(40, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.Reconnect)).ToInt32());
        Assert.Equal(44, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.MaxReconnectAttempts)).ToInt32());
        Assert.Equal(48, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.CookieHeader)).ToInt32());
        Assert.Equal(56, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.CookieHeaderLen)).ToInt32());

        Assert.Equal(48, Marshal.SizeOf<NativeErrorInfo>());
        Assert.Equal(0, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.Code)).ToInt32());
        Assert.Equal(8, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.Message)).ToInt32());
        Assert.Equal(16, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.MessageLen)).ToInt32());
        Assert.Equal(24, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.Retryable)).ToInt32());
        Assert.Equal(32, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.RetryAfterMs)).ToInt32());
        Assert.Equal(40, Marshal.OffsetOf<NativeErrorInfo>(nameof(NativeErrorInfo.HttpStatus)).ToInt32());
        Assert.Equal(88, Marshal.SizeOf<NativeEventInfo>());
        Assert.Equal(0, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.AbiVersion)).ToInt32());
        Assert.Equal(8, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.Method)).ToInt32());
        Assert.Equal(16, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.MethodLen)).ToInt32());
        Assert.Equal(24, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.MessageId)).ToInt32());
        Assert.Equal(32, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.Support)).ToInt32());
        Assert.Equal(36, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.IsHistory)).ToInt32());
        Assert.Equal(40, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.SchemaName)).ToInt32());
        Assert.Equal(48, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.SchemaNameLen)).ToInt32());
        Assert.Equal(56, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.RawPayloadSize)).ToInt32());
        Assert.Equal(64, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.MethodId)).ToInt32());
        Assert.Equal(72, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.Timestamp)).ToInt32());
        Assert.Equal(80, Marshal.OffsetOf<NativeEventInfo>(nameof(NativeEventInfo.HasTimestamp)).ToInt32());
        Assert.Equal(72, Marshal.SizeOf<NativeUser>());
        Assert.Equal(0, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.Id)).ToInt32());
        Assert.Equal(8, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.Nickname)).ToInt32());
        Assert.Equal(16, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.NicknameLen)).ToInt32());
        Assert.Equal(24, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.UniqueId)).ToInt32());
        Assert.Equal(32, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.UniqueIdLen)).ToInt32());
        Assert.Equal(40, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.SecUid)).ToInt32());
        Assert.Equal(48, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.SecUidLen)).ToInt32());
        Assert.Equal(56, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.AvatarUrl)).ToInt32());
        Assert.Equal(64, Marshal.OffsetOf<NativeUser>(nameof(NativeUser.AvatarUrlLen)).ToInt32());
        Assert.Equal(88, Marshal.SizeOf<NativeChatEvent>());
        Assert.Equal(0, Marshal.OffsetOf<NativeChatEvent>(nameof(NativeChatEvent.User)).ToInt32());
        Assert.Equal(72, Marshal.OffsetOf<NativeChatEvent>(nameof(NativeChatEvent.Comment)).ToInt32());
        Assert.Equal(80, Marshal.OffsetOf<NativeChatEvent>(nameof(NativeChatEvent.CommentLen)).ToInt32());
        Assert.Equal(152, Marshal.SizeOf<NativeGiftEvent>());
        Assert.Equal(0, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.User)).ToInt32());
        Assert.Equal(72, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GiftId)).ToInt32());
        Assert.Equal(80, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GiftName)).ToInt32());
        Assert.Equal(88, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GiftNameLen)).ToInt32());
        Assert.Equal(96, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.DiamondCount)).ToInt32());
        Assert.Equal(104, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.RepeatCount)).ToInt32());
        Assert.Equal(112, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.ComboCount)).ToInt32());
        Assert.Equal(120, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GroupId)).ToInt32());
        Assert.Equal(128, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.RepeatEnd)).ToInt32());
        Assert.Equal(136, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GiftImageUrl)).ToInt32());
        Assert.Equal(144, Marshal.OffsetOf<NativeGiftEvent>(nameof(NativeGiftEvent.GiftImageUrlLen)).ToInt32());
    }

    [Fact]
    public void ManagedEnumValuesMatchTheCAbiValues()
    {
        Assert.Equal(Enumerable.Range(0, 14), Enum.GetValues<NativeResultCode>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(0, 6), Enum.GetValues<ConnectionState>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(1, 3), Enum.GetValues<EventSupport>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(0, 7), Enum.GetValues<TtlValueType>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(0, 18), Enum.GetValues<ProtoType>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(0, 2), Enum.GetValues<FieldCardinality>().Select(value => (int)value));
        Assert.Equal(Enumerable.Range(0, 5), Enum.GetValues<JsonMode>().Select(value => (int)value));
    }
}
