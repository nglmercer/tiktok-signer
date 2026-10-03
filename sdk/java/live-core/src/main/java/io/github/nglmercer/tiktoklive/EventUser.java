package io.github.nglmercer.tiktoklive;

public record EventUser(
        String userId, String nickname, String uniqueId, String secUid, String avatarUrl) {}
