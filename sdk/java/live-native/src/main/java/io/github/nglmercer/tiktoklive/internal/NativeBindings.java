package io.github.nglmercer.tiktoklive.internal;

/** Private implementation boundary; applications use TikTokSigner. */
public final class NativeBindings {
    private NativeBindings() {}

    public static native long nOpen(String bundle, String optionsJson);

    public static native String nSign(long handle, String url, String product);

    public static native void nClose(long handle);
}
