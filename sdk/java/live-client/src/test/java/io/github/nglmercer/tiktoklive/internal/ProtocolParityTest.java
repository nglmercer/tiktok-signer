package io.github.nglmercer.tiktoklive.internal;

import static org.junit.jupiter.api.Assertions.*;

import com.fasterxml.jackson.databind.*;
import com.google.protobuf.ByteString;

import io.github.nglmercer.tiktoklive.*;

import org.junit.jupiter.api.Test;

import webcast.im.SyntheticProto.*;
import webcast.shared.message.Message.*;

import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;

class ProtocolParityTest {
    static final Path ROOT = Path.of(System.getProperty("repositoryRoot"));
    static final Path FIXTURES = ROOT.resolve("fixtures/events");
    static final ObjectMapper JSON = new ObjectMapper();

    byte[] read(String name) throws IOException {
        return Files.readAllBytes(FIXTURES.resolve(name));
    }

    @Test
    void transportOracle() throws Exception {
        var f = JSON.readTree(read("transport-parity.json"));
        var hex = HexFormat.of();
        var incoming = Protocol.decode(hex.parseHex(f.path("incoming").asText()));
        assertEquals(f.path("logId").asText(), Long.toString(incoming.getLogId()));
        assertArrayEquals(
                hex.parseHex(f.path("enter").asText()), Protocol.enter(f.path("roomId").asText()));
        assertArrayEquals(
                hex.parseHex(f.path("heartbeat").asText()),
                Protocol.heartbeat(f.path("roomId").asText()));
        assertArrayEquals(
                hex.parseHex(f.path("ack").asText()), Protocol.ack(incoming, "fixture-ext"));
        assertArrayEquals(hex.parseHex(f.path("ackEmpty").asText()), Protocol.ack(incoming, ""));
        assertFalse(Protocol.batch(incoming).getMessagesList().isEmpty());
    }

    @Test
    void gzipAndCorruptFrames() throws Exception {
        var out = new ByteArrayOutputStream();
        try (var gzip = new GZIPOutputStream(out)) {
            gzip.write(read("batch.pb"));
        }
        var frame =
                WebcastPushFrame.newBuilder()
                        .setPayloadType("msg")
                        .addHeaders(
                                PushHeader.newBuilder().setKey("compress_type").setValue("gzip"))
                        .setPayload(ByteString.copyFrom(out.toByteArray()))
                        .build();
        assertEquals(ProtoMessageFetchResult.parseFrom(read("batch.pb")), Protocol.batch(frame));
        assertThrows(
                IOException.class,
                () ->
                        Protocol.batch(
                                frame.toBuilder()
                                        .setPayload(ByteString.copyFromUtf8("broken"))
                                        .build()));
        assertThrows(IOException.class, () -> Protocol.decode(new byte[] {(byte) 255}));
    }

    JsonNode golden(LiveEvent event) {
        var o = JSON.createObjectNode();
        o.put("type", event.type().equals("roomUser") ? "room_user" : event.type());
        EventUser user = null;
        if (event instanceof ChatEvent e) {
            user = e.user();
            o.put("comment", e.comment());
        }
        if (event instanceof GiftEvent e) {
            user = e.user();
            o.put("gift_id", e.giftId());
            o.put("gift_name", e.giftName());
            o.put("diamond_count", Long.toString(e.diamondCount()));
            o.put("repeat_count", Long.toString(e.repeatCount()));
            o.put("combo_count", Long.toString(e.comboCount()));
            o.put("group_id", e.groupId());
            o.put("repeat_end", e.repeatEnd());
        }
        if (event instanceof LikeEvent e) {
            user = e.user();
            o.put("count", Long.toString(e.count()));
            o.put("total", Long.toString(e.total()));
        }
        if (event instanceof MemberEvent e) {
            user = e.user();
            o.put("member_count", Long.toString(e.memberCount()));
            o.put("action", Long.toString(e.action()));
        }
        if (event instanceof SocialEvent e) {
            user = e.user();
            o.put("action", Long.toString(e.action()));
            o.put("follow_count", Long.toString(e.followCount()));
            o.put("share_count", Long.toString(e.shareCount()));
        }
        if (event instanceof RoomUserEvent e) {
            o.put("total", Long.toString(e.viewers()));
            o.put("popularity", Long.toString(e.popularity()));
            o.put("total_user", Long.toString(e.totalUser()));
            o.put("anonymous", Long.toString(e.anonymous()));
        }
        if (user != null) {
            var u = o.putObject("user");
            u.put("id", user.userId());
            u.put("nickname", user.nickname());
            u.put("unique_id", user.uniqueId());
            u.put("sec_uid", user.secUid());
        }
        return o;
    }

    @Test
    void nodeAndRustGoldenEvents() throws Exception {
        String[] files = {"chat", "gift", "like", "member", "social", "room-user"};
        String[] methods = {
            "WebcastChatMessage",
            "WebcastGiftMessage",
            "WebcastLikeMessage",
            "WebcastMemberMessage",
            "WebcastSocialMessage",
            "WebcastRoomUserSeqMessage"
        };
        for (int i = 0; i < files.length; i++) {
            var envelope =
                    BaseProtoMessage.newBuilder()
                            .setMethod(methods[i])
                            .setMsgId(9007199254740993L)
                            .setIsHistory(true)
                            .setPayload(ByteString.copyFrom(read(files[i] + ".pb")))
                            .build();
            var event = EventDecoder.decode(envelope);
            assertEquals(
                    JSON.readTree(read("expected/" + files[i] + ".json")), golden(event), files[i]);
            assertEquals("9007199254740993", event.msgId());
            assertTrue(event.isHistory());
        }
    }

    @Test
    void malformedEventDoesNotKillBatch() {
        for (String method : new String[] {"WebcastChatMessage", "FutureMessage"}) {
            var e =
                    EventDecoder.decode(
                            BaseProtoMessage.newBuilder()
                                    .setMethod(method)
                                    .setPayload(ByteString.copyFrom(new byte[] {(byte) 255}))
                                    .build());
            assertInstanceOf(UnknownEvent.class, e);
        }
        assertEquals(0, EventDecoder.count(-1));
        assertEquals(9007199254740991L, EventDecoder.count(Long.MAX_VALUE));
    }

    @Test
    void giftEnrichment() throws Exception {
        var map =
                GiftCatalog.parse(
                        JSON.readTree(
                                "{\"status_code\":0,\"data\":{\"gifts\":[{\"id\":5655,\"name\":\"Rose\",\"diamond_count\":1,\"combo\":true,\"icon\":{\"url_list\":[\"icon\"]}}]}}"));
        var e =
                (GiftEvent)
                        EventDecoder.decode(
                                BaseProtoMessage.newBuilder()
                                        .setMethod("WebcastGiftMessage")
                                        .setPayload(ByteString.copyFrom(read("gift.pb")))
                                        .build());
        var enriched = GiftCatalog.enrich(e, map);
        assertEquals("icon", enriched.iconUrl());
        assertEquals(true, enriched.streakable());
    }
}
