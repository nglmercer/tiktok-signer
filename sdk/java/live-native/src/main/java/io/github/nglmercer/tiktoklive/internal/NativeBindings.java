package io.github.nglmercer.tiktoklive.internal;

/** Private implementation boundary; applications use TikTokSigner. */
public final class NativeBindings {
    private NativeBindings() {}

    public static native long nOpen(String bundle, String optionsJson);

    public static native String nSign(long handle, String url, String product);

    /** Interrupts the running call, if any; later calls fail. Never races {@link #nClose}. */
    public static native void nInterrupt(long handle);

    public static native void nClose(long handle);
}
