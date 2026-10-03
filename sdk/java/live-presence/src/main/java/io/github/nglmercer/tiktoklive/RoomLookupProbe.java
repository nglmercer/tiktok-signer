package io.github.nglmercer.tiktoklive;

import com.fasterxml.jackson.databind.*;

import okhttp3.*;

import java.io.IOException;
import java.util.Set;
import java.util.concurrent.*;

/** Unsigned public lookup. Failures and schema changes remain UNKNOWN. */
public final class RoomLookupProbe implements PresenceProbe, AutoCloseable {
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final String USER_AGENT =
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)"
                    + " Chrome/131.0.0.0 Safari/537.36";
    private final OkHttpClient http;
    private final HttpUrl endpoint;
    private final Set<Call> calls = ConcurrentHashMap.newKeySet();
    private volatile boolean closed;

    public RoomLookupProbe(OkHttpClient http) {
        this(http, "https://www.tiktok.com/api-live/user/room/");
    }

    /**
     * Alternate endpoint is useful for deterministic local tests and compatible discovery proxies.
     */
    public RoomLookupProbe(OkHttpClient http, String endpoint) {
        this.http = http;
        this.endpoint = HttpUrl.get(endpoint);
    }

    public CompletableFuture<LivePresence> lookup(String input) {
        String id = UniqueId.normalize(input);
        var result = new CompletableFuture<LivePresence>();
        var url =
                endpoint.newBuilder()
                        .addQueryParameter("aid", "1988")
                        .addQueryParameter("sourceType", "54")
                        .addQueryParameter("uniqueId", id)
                        .build();
        Call call =
                http.newCall(
                        new Request.Builder()
                                .url(url)
                                .header("User-Agent", USER_AGENT)
                                .header("Referer", "https://www.tiktok.com/")
                                .build());
        synchronized (this) {
            if (closed)
                return CompletableFuture.failedFuture(new IllegalStateException("Probe closed"));
            calls.add(call);
        }
        result.whenComplete(
                (v, e) -> {
                    calls.remove(call);
                    if (result.isCancelled()) call.cancel();
                });
        call.enqueue(
                new Callback() {
                    public void onFailure(Call call, IOException error) {
                        result.complete(LivePresence.unknown(id));
                    }

                    public void onResponse(Call call, Response response) {
                        try (response) {
                            result.complete(
                                    parse(
                                            id,
                                            response.code(),
                                            response.body() == null
                                                    ? ""
                                                    : response.body().string()));
                        } catch (Exception e) {
                            result.complete(LivePresence.unknown(id));
                        }
                    }
                });
        return result;
    }

    public static LivePresence parse(String input, int httpStatus, String body) {
        String id = UniqueId.normalize(input);
        if (httpStatus < 200 || httpStatus >= 300) return LivePresence.unknown(id);
        try {
            var root = JSON.readTree(body);
            if (root == null || !root.isObject()) return LivePresence.unknown(id);
            if (root.has("status_code")
                    && (!root.path("status_code").isIntegralNumber()
                            || root.path("status_code").asLong() != 0))
                return LivePresence.unknown(id);
            if (root.has("captcha")
                    || root.has("challenge")
                    || root.has("verify_center_decision_conf")) return LivePresence.unknown(id);
            var user = root.path("data").path("user");
            var room = root.path("data").path("liveRoom");
            if (!user.isObject() || (!room.isMissingNode() && !room.isNull() && !room.isObject()))
                return LivePresence.unknown(id);
            for (String name : new String[] {"uniqueId", "nickname"})
                if (user.has(name) && !user.path(name).isTextual()) return LivePresence.unknown(id);
            if (room.has("title") && !room.path("title").isTextual())
                return LivePresence.unknown(id);
            if (user.has("uniqueId") && !user.path("uniqueId").asText().equals(id))
                return LivePresence.unknown(id);
            JsonNode status = user.has("status") ? user.path("status") : room.path("status");
            if (!status.isIntegralNumber() || !status.canConvertToInt())
                return LivePresence.unknown(id);
            int value = status.asInt();
            if (value != 0 && value != 2 && value != 4) return LivePresence.unknown(id);
            JsonNode raw = user.path("roomId");
            String roomId = "";
            if (!raw.isMissingNode() && !raw.isNull()) {
                if (raw.isTextual()) {
                    roomId = raw.asText();
                    if (!roomId.isEmpty() && !roomId.matches("[0-9]+"))
                        return LivePresence.unknown(id);
                } else if (raw.isIntegralNumber()
                        && raw.canConvertToLong()
                        && raw.asLong() >= 0
                        && raw.asLong() <= 9007199254740991L) roomId = raw.asText();
                else return LivePresence.unknown(id);
            }
            return new LivePresence(
                    id,
                    roomId,
                    user.path("nickname").asText(""),
                    room.path("title").asText(""),
                    value == 2 && !roomId.isEmpty() && !roomId.equals("0")
                            ? PresenceStatus.LIVE
                            : PresenceStatus.OFFLINE);
        } catch (Exception e) {
            return LivePresence.unknown(id);
        }
    }

    public synchronized void close() {
        if (closed) return;
        closed = true;
        calls.forEach(Call::cancel);
    }
}
