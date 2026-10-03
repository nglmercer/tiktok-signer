package io.github.nglmercer.tiktoklive;

public final class UniqueId {
    private UniqueId() {}

    public static String normalize(String input) {
        if (input == null) throw new IllegalArgumentException("uniqueId is required");
        String id = input.trim();
        if (id.startsWith("@")) id = id.substring(1);
        if (!id.matches("[A-Za-z0-9._]{1,24}"))
            throw new IllegalArgumentException("uniqueId must be a TikTok username");
        return id;
    }
}
