using System.Runtime.InteropServices;
using Ttl.Live.Native;

namespace Ttl.Live.Tests;

public sealed class NativeAbiLayoutTests
{
    [Fact]
    public void CStructsMatchThe64BitPublicHeaderLayouts()
    {
        Assert.Equal(64, Marshal.SizeOf<NativeLiveOptions>());
        Assert.Equal(0, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.StructSize)).ToInt32());
        Assert.Equal(8, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.UniqueId)).ToInt32());
        Assert.Equal(16, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.UniqueIdLen)).ToInt32());
        Assert.Equal(24, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.Bundle)).ToInt32());
        Assert.Equal(32, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.BundleLen)).ToInt32());
        Assert.Equal(40, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.Reconnect)).ToInt32());
        Assert.Equal(44, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.MaxReconnectAttempts)).ToInt32());
        Assert.Equal(48, Marshal.OffsetOf<NativeLiveOptions>(nameof(NativeLiveOptions.CookieHeader)).ToInt32());

        Assert.Equal(48, Marshal.SizeOf<NativeErrorInfo>());
        Assert.Equal(88, Marshal.SizeOf<NativeEventInfo>());
        Assert.Equal(72, Marshal.SizeOf<NativeUser>());
        Assert.Equal(88, Marshal.SizeOf<NativeChatEvent>());
        Assert.Equal(152, Marshal.SizeOf<NativeGiftEvent>());
    }
}
