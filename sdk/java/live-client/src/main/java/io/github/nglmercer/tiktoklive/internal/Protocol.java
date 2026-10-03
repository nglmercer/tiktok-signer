package io.github.nglmercer.tiktoklive.internal;

import com.google.protobuf.ByteString;

import webcast.im.SyntheticProto.*;
import webcast.shared.message.Message.ProtoMessageFetchResult;

import java.io.*;
import java.util.zip.GZIPInputStream;

public final class Protocol {
    private Protocol() {}

    public static WebcastPushFrame decode(byte[] bytes) throws IOException {
        return WebcastPushFrame.parseFrom(bytes);
    }

    public static ProtoMessageFetchResult batch(WebcastPushFrame frame) throws IOException {
        byte[] data = frame.getPayload().toByteArray();
        boolean gzip =
                frame.getHeadersList().stream()
                        .anyMatch(
                                h ->
                                        h.getKey().equals("compress_type")
                                                && h.getValue().equals("gzip"));
        if (gzip)
            try (var input = new GZIPInputStream(new ByteArrayInputStream(data))) {
                data = input.readNBytes(16 * 1024 * 1024 + 1);
                if (data.length > 16 * 1024 * 1024)
                    throw new IOException("Decompressed batch too large");
            }
        return ProtoMessageFetchResult.parseFrom(data);
    }

    private static byte[] frame(String type, byte[] payload, long logId) {
        return WebcastPushFrame.newBuilder()
                .setLogId(logId)
                .setPayloadEncoding("pb")
                .setPayloadType(type)
                .setPayload(ByteString.copyFrom(payload))
                .build()
                .toByteArray();
    }

    public static byte[] ack(WebcastPushFrame frame, String ext) {
        return frame(
                "ack",
                (ext.isEmpty() ? "-" : ext).getBytes(java.nio.charset.StandardCharsets.UTF_8),
                frame.getLogId());
    }

    public static byte[] enter(String room) {
        return frame(
                "im_enter_room",
                WebcastImEnterRoomMessage.newBuilder()
                        .setRoomId(id(room))
                        .setLiveId(12)
                        .setIdentity("audience")
                        .setFilterWelcomeMsg("0")
                        .build()
                        .toByteArray(),
                0);
    }

    public static byte[] heartbeat(String room) {
        return frame(
                "hb", HeartBeatMessage.newBuilder().setRoomId(id(room)).build().toByteArray(), 0);
    }

    private static long id(String room) {
        long value = Long.parseLong(room);
        if (value < 0) throw new IllegalArgumentException("Negative room id");
        return value;
    }
}
