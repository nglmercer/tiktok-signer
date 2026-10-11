using System.Net;

public static class OperatorHttpEndpoints
{
    public static bool IsLocalOperator(HttpContext context)
    {
        IPAddress? address = context.Connection.RemoteIpAddress;
        if (address is null || !IPAddress.IsLoopback(address)) return false;
        string host = context.Request.Host.Host.Trim('[', ']');
        if (host != "localhost" && host != "127.0.0.1" && host != "::1") return false;
        string origin = context.Request.Headers.Origin.ToString();
        return string.IsNullOrEmpty(origin) ||
            string.Equals(origin, $"{context.Request.Scheme}://{context.Request.Host}", StringComparison.OrdinalIgnoreCase);
    }

    public static void Map(WebApplication app)
    {
        app.MapGet("/", (HttpContext context) => IsLocalOperator(context)
            ? Results.Content(OperatorPage.Html, "text/html; charset=utf-8")
            : Results.StatusCode(403));
        app.MapGet("/operator/status", (HttpContext context, InteractiveLiveSession session) =>
            IsLocalOperator(context) ? Results.Json(session.Status) : Results.StatusCode(403));
        app.MapPost("/operator/connect", async (HttpContext context, InteractiveLiveSession session) =>
        {
            if (!IsLocalOperator(context)) return Results.StatusCode(403);
            if (!context.Request.HasJsonContentType()) return Results.StatusCode(415);
            var limit = context.Features.Get<Microsoft.AspNetCore.Http.Features.IHttpMaxRequestBodySizeFeature>();
            if (limit is not null && !limit.IsReadOnly) limit.MaxRequestBodySize = 1024;
            try
            {
                CreatorRequest? request = await context.Request.ReadFromJsonAsync<CreatorRequest>(context.RequestAborted);
                string creator = InteractiveLiveSession.NormalizeCreator(request?.Creator);
                if (!string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable("LIVE_CREATORS")))
                    return Results.Conflict(new { error = "Unset LIVE_CREATORS before using interactive creator selection." });
                await session.ConnectAsync(creator);
                return Results.Json(session.Status, statusCode: 202);
            }
            catch (ArgumentException)
            { return Results.BadRequest(new { error = "Invalid username or missing trusted TTL_BUNDLE configuration." }); }
            catch (IOException) { return Results.BadRequest(new { error = "Server session file is unavailable." }); }
            catch (DllNotFoundException) { return Results.Problem("Native library is unavailable on the relay host.", statusCode: 503); }
            catch (System.Text.Json.JsonException) { return Results.BadRequest(new { error = "Invalid JSON." }); }
        });
        app.MapPost("/operator/disconnect", async (HttpContext context, InteractiveLiveSession session) =>
        {
            if (!IsLocalOperator(context)) return Results.StatusCode(403);
            if (!context.Request.HasJsonContentType()) return Results.StatusCode(415);
            await session.DisconnectAsync();
            return Results.Json(session.Status);
        });
        app.MapPost("/operator/fixtures", async (HttpContext context, InteractiveLiveSession session) =>
        {
            if (!IsLocalOperator(context)) return Results.StatusCode(403);
            if (!context.Request.HasJsonContentType()) return Results.StatusCode(415);
            string? directory = Environment.GetEnvironmentVariable("LIVE_FIXTURES_DIRECTORY");
            if (string.IsNullOrWhiteSpace(directory)) return Results.BadRequest(new { error = "Fixture mode is not configured on this relay." });
            try { await session.LoadFixturesAsync(directory); }
            catch (Exception error) when (error is IOException or ArgumentException)
            { return Results.BadRequest(new { error = "Fixture files or native library are unavailable." }); }
            return Results.Json(session.Status);
        });
        app.MapGet("/vrchat/live", (HttpContext context, LiveEventFeed feed, InteractiveLiveSession session) =>
        {
            if (!feed.IsIpAllowed(context.Connection.RemoteIpAddress)) return Results.StatusCode(403);
            context.Response.Headers.CacheControl = "no-store";
            context.Response.Headers["X-Content-Type-Options"] = "nosniff";
            return Results.Json(session.Snapshot());
        });
    }
    private sealed record CreatorRequest(string? Creator);
}
