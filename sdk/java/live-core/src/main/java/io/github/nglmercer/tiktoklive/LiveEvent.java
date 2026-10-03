package io.github.nglmercer.tiktoklive;

public sealed interface LiveEvent
        permits ChatEvent,
                GiftEvent,
                LikeEvent,
                MemberEvent,
                SocialEvent,
                RoomUserEvent,
                UnknownEvent {
    String type();

    String method();

    String msgId();

    boolean isHistory();
}
