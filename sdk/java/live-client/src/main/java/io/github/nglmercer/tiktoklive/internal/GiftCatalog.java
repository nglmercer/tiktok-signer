package io.github.nglmercer.tiktoklive.internal;

import com.fasterxml.jackson.databind.JsonNode;

import io.github.nglmercer.tiktoklive.*;

import okhttp3.*;

import java.util.*;
import java.util.concurrent.CompletableFuture;

public final class GiftCatalog {
    public record Gift(String name, long diamonds, String icon, boolean streakable) {}

    public static CompletableFuture<Map<String, Gift>> load(
            ConnectApi api, String room, ConnectionDescriptor descriptor) {
        var url =
                HttpUrl.get("https://webcast.tiktok.com/webcast/gift/list/")
                        .newBuilder()
                        .addQueryParameter("aid", "1988")
                        .addQueryParameter("app_language", "en")
                        .addQueryParameter("device_platform", "web")
                        .addQueryParameter("room_id", room)
                        .build();
        return api.json(
                        new Request.Builder()
                                .url(url)
                                .header("User-Agent", descriptor.userAgent())
                                .header("Cookie", descriptor.cookies())
                                .header("Referer", "https://www.tiktok.com/")
                                .build())
                .thenApply(GiftCatalog::parse);
    }

    static Map<String, Gift> parse(JsonNode body) {
        if (!body.path("status_code").isIntegralNumber()
                || body.path("status_code").asInt() != 0
                || !body.path("data").path("gifts").isArray())
            throw new ProtocolException("Invalid gift catalog");
        Map<String, Gift> gifts = new HashMap<>();
        for (var g : body.path("data").path("gifts")) {
            String id = g.path("id").asText();
            if (!id.matches("[0-9]+")) continue;
            gifts.put(
                    id,
                    new Gift(
                            g.path("name").asText(""),
                            EventDecoder.count(g.path("diamond_count").asLong()),
                            g.path("icon").path("url_list").path(0).asText(""),
                            g.path("combo").asBoolean(false)));
        }
        return Map.copyOf(gifts);
    }

    public static GiftEvent enrich(GiftEvent e, Map<String, Gift> catalog) {
        var g = catalog.get(e.giftId());
        if (g == null) return e;
        return new GiftEvent(
                e.method(),
                e.msgId(),
                e.isHistory(),
                e.user(),
                e.toUser(),
                e.giftId(),
                e.giftName().isEmpty() ? g.name() : e.giftName(),
                e.diamondCount() == 0 ? g.diamonds() : e.diamondCount(),
                e.repeatCount(),
                e.comboCount(),
                e.groupId(),
                e.repeatEnd(),
                g.icon(),
                g.streakable());
    }
}
