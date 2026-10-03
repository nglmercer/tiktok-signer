package io.github.nglmercer.tiktoklive.internal;

import io.github.nglmercer.tiktoklive.*;

import webcast.model.base.Messages.ImageModel;
import webcast.model.base.user.User2.User;
import webcast.model.message.Messages.*;
import webcast.shared.message.Message.BaseProtoMessage;

import java.util.List;

public final class EventDecoder {
    private EventDecoder() {}

    public static long count(long value) {
        return Math.min(9007199254740991L, Math.max(0, value));
    }

    private static String image(ImageModel image) {
        return image.getUrlListCount() == 0 ? "" : image.getUrlList(0);
    }

    private static EventUser user(User u) {
        String avatar = image(u.getAvatarThumb());
        if (avatar.isEmpty()) avatar = image(u.getAvatarMedium());
        if (avatar.isEmpty()) avatar = image(u.getAvatarLarge());
        if (avatar.isEmpty()) avatar = image(u.getAvatarJpg());
        return new EventUser(
                Long.toString(u.getId()), u.getNickname(), u.getDisplayId(), u.getSecUid(), avatar);
    }

    public static LiveEvent decode(BaseProtoMessage envelope) {
        String method = envelope.getMethod(), id = Long.toString(envelope.getMsgId());
        boolean history = envelope.getIsHistory();
        var payload = envelope.getPayload();
        try {
            return switch (method) {
                case "WebcastChatMessage" -> {
                    var m = WebcastChatMessage.parseFrom(payload);
                    yield new ChatEvent(method, id, history, user(m.getUser()), m.getContent());
                }
                case "WebcastGiftMessage" -> {
                    var m = WebcastGiftMessage.parseFrom(payload);
                    var g = m.getGift();
                    yield new GiftEvent(
                            method,
                            id,
                            history,
                            user(m.getUser()),
                            user(m.getToUser()),
                            Long.toString(m.getGiftId()),
                            g.getName(),
                            count(g.getDiamondCount()),
                            count(m.getRepeatCount()),
                            count(m.getComboCount()),
                            Long.toString(m.getGroupId()),
                            m.getRepeatEnd() > 0,
                            "",
                            null);
                }
                case "WebcastLikeMessage" -> {
                    var m = WebcastLikeMessage.parseFrom(payload);
                    yield new LikeEvent(
                            method,
                            id,
                            history,
                            user(m.getUser()),
                            count(m.getCount()),
                            count(m.getTotal()));
                }
                case "WebcastMemberMessage" -> {
                    var m = WebcastMemberMessage.parseFrom(payload);
                    yield new MemberEvent(
                            method,
                            id,
                            history,
                            user(m.getUser()),
                            count(m.getMemberCount()),
                            count(m.getActionValue()));
                }
                case "WebcastSocialMessage" -> {
                    var m = WebcastSocialMessage.parseFrom(payload);
                    yield new SocialEvent(
                            method,
                            id,
                            history,
                            user(m.getUser()),
                            count(m.getAction()),
                            count(m.getFollowCount()),
                            count(m.getShareCount()));
                }
                case "WebcastRoomUserSeqMessage" -> {
                    var m = WebcastRoomUserSeqMessage.parseFrom(payload);
                    List<TopViewer> ranks =
                            m.getRanksList().stream()
                                    .map(
                                            r ->
                                                    new TopViewer(
                                                            count(r.getRank()),
                                                            count(r.getScore()),
                                                            count(r.getDelta()),
                                                            user(r.getUser())))
                                    .toList();
                    yield new RoomUserEvent(
                            method,
                            id,
                            history,
                            count(m.getTotal()),
                            count(m.getPopularity()),
                            count(m.getTotalUser()),
                            count(m.getAnonymous()),
                            ranks);
                }
                default -> new UnknownEvent(method, id, history, payload.toByteArray());
            };
        } catch (Exception malformed) {
            return new UnknownEvent(method, id, history, payload.toByteArray());
        }
    }
}
