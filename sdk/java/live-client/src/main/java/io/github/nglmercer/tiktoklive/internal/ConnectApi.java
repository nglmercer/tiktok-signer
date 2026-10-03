package io.github.nglmercer.tiktoklive.internal;

import com.fasterxml.jackson.databind.*;

import io.github.nglmercer.tiktoklive.*;

import okhttp3.*;

import java.io.IOException;
import java.net.URI;
import java.util.Set;
import java.util.concurrent.*;

public final class ConnectApi implements AutoCloseable {
    private static final ObjectMapper JSON = new ObjectMapper();
    private final OkHttpClient http;
    private final HttpUrl base;
    private final String key;
    private final Set<Call> pending = ConcurrentHashMap.newKeySet();
    private volatile boolean closed;

    public ConnectApi(OkHttpClient http, String base, String key) {
        this.http = http;
        this.base = HttpUrl.get(base);
        this.key = key;
        if (!this.base.username().isEmpty() || !this.base.password().isEmpty())
            throw new IllegalArgumentException("API URL must not contain credentials");
    }

    public CompletableFuture<ConnectResponse> connect(String uniqueId) {
        String id = UniqueId.normalize(uniqueId);
        var builder =
                new Request.Builder()
                        .url(
                                base.newBuilder()
                                        .encodedPath("/v1/connect")
                                        .query(null)
                                        .fragment(null)
                                        .build())
                        .header("Accept", "application/json")
                        .post(
                                RequestBody.create(
                                        "{\"uniqueId\":\"" + id + "\"}",
                                        MediaType.get("application/json")));
        if (key != null && !key.isEmpty()) builder.header("Authorization", "Bearer " + key);
        return json(builder.build()).thenApply(body -> parse(body, id));
    }

    public CompletableFuture<JsonNode> json(Request request) {
        var result = new CompletableFuture<JsonNode>();
        Call call = http.newCall(request);
        synchronized (this) {
            if (closed)
                return CompletableFuture.failedFuture(new IllegalStateException("Client closed"));
            pending.add(call);
        }
        result.whenComplete(
                (v, e) -> {
                    pending.remove(call);
                    if (result.isCancelled()) call.cancel();
                });
        call.enqueue(
                new Callback() {
                    public void onFailure(Call call, IOException error) {
                        var failure =
                                new DiscoveryException(
                                        "HTTP request failed: " + error.getMessage());
                        failure.initCause(error);
                        result.completeExceptionally(failure);
                    }

                    public void onResponse(Call call, Response response) {
                        try (response) {
                            JsonNode body;
                            try {
                                body =
                                        JSON.readTree(
                                                response.body() == null
                                                        ? ""
                                                        : response.body().string());
                            } catch (Exception e) {
                                if (!response.isSuccessful())
                                    throw error(
                                            response.code(), response.header("x-request-id"), null);
                                throw new ProtocolException("Invalid JSON response");
                            }
                            if (!response.isSuccessful())
                                throw error(response.code(), response.header("x-request-id"), body);
                            if (body == null) throw new ProtocolException("Empty JSON response");
                            result.complete(body);
                        } catch (Exception e) {
                            result.completeExceptionally(e);
                        }
                    }
                });
        return result;
    }

    public static ConnectResponse parse(JsonNode body, String id) {
        if (!body.isObject()
                || !body.path("version").isIntegralNumber()
                || body.path("version").asInt() != 1
                || !body.path("uniqueId").isTextual()
                || !body.path("uniqueId").asText().equals(id))
            throw new ProtocolException("Mismatched connection metadata");
        String status = body.path("status").asText();
        String room = body.path("roomId").isTextual() ? body.path("roomId").asText() : "";
        if (status.equals("offline"))
            return new ConnectResponse(
                    1, id, room.matches("[0-9]+") ? room : "", ConnectionStatus.OFFLINE, null);
        if (!status.equals("live") || !room.matches("[0-9]+") || room.equals("0"))
            throw new ProtocolException("Invalid live connection response");
        try {
            if (Long.parseLong(room) <= 0) throw new NumberFormatException();
        } catch (NumberFormatException e) {
            throw new ProtocolException("Room id outside protocol int64 range");
        }
        var c = body.path("connection");
        if (!c.path("url").isTextual()
                || !c.path("cookies").isTextual()
                || !c.path("userAgent").isTextual()
                || !allowedSocket(c.path("url").asText(), room))
            throw new ProtocolException("Invalid connection descriptor");
        return new ConnectResponse(
                1,
                id,
                room,
                ConnectionStatus.LIVE,
                new ConnectionDescriptor(
                        c.path("url").asText(),
                        c.path("cookies").asText(),
                        c.path("userAgent").asText()));
    }

    public static boolean allowedSocket(String url, String room) {
        try {
            var uri = URI.create(url);
            var parsed = HttpUrl.get(url.replaceFirst("^wss:", "https:"));
            return uri.getScheme().equals("wss")
                    && uri.getRawUserInfo() == null
                    && uri.getFragment() == null
                    && (uri.getPort() == -1 || uri.getPort() == 443)
                    && Set.of(
                                    "webcast-ws.tiktok.com",
                                    "webcast-ws.us.tiktok.com",
                                    "webcast-ws.eu.tiktok.com")
                            .contains(uri.getHost())
                    && uri.getPath().equals("/webcast/im/ws_proxy/ws_reuse_supplement/")
                    && room.equals(parsed.queryParameter("room_id"))
                    && parsed.queryParameter("X-Gnarly") != null
                    && !parsed.queryParameter("X-Gnarly").isEmpty();
        } catch (Exception e) {
            return false;
        }
    }

    public static TikTokLiveException error(int status, String requestId, JsonNode body) {
        var e = body == null ? JSON.createObjectNode() : body.path("error");
        String code = e.path("code").asText(status == 429 ? "RATE_LIMITED" : "INTERNAL_ERROR");
        String message = e.path("message").asText("HTTP " + status);
        boolean retry =
                e.path("retryable").isBoolean()
                        ? e.path("retryable").asBoolean()
                        : status >= 500 || status == 429;
        Long after =
                e.path("retryAfterMs").isIntegralNumber()
                                && e.path("retryAfterMs").canConvertToLong()
                        ? Math.max(0, e.path("retryAfterMs").asLong())
                        : null;
        return switch (code) {
            case "AUTHENTICATION_REQUIRED", "INVALID_API_KEY" ->
                    new AuthenticationException(code, message, retry, after, status, requestId);
            case "RATE_LIMITED" ->
                    new RateLimitException(code, message, retry, after, status, requestId);
            case "SIGNER_UNAVAILABLE", "SIGN_FAILED" ->
                    new SignerUnavailableException(code, message, retry, after, status, requestId);
            case "DISCOVERY_FAILED", "USER_NOT_FOUND", "TIKTOK_REFUSED" ->
                    new DiscoveryException(code, message, retry, after, status, requestId);
            default ->
                    status == 401 || status == 403
                            ? new AuthenticationException(
                                    code, message, retry, after, status, requestId)
                            : status == 429
                                    ? new RateLimitException(
                                            code, message, retry, after, status, requestId)
                                    : new ApiException(
                                            code, message, retry, after, status, requestId);
        };
    }

    public synchronized void cancelPending() {
        for (Call c : pending) c.cancel();
    }

    public synchronized void close() {
        closed = true;
        cancelPending();
    }
}
