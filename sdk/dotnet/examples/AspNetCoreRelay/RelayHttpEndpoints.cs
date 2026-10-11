using Ttl.Live;

public static class RelayHttpEndpoints
{
    public static void Map(WebApplication app)
    {
        app.MapGet("/health", () => Results.Ok(new { status = "running" }));
        app.MapGet("/events", async (HttpContext context, LiveEventFeed feed) =>
        {
            context.Response.ContentType = "text/event-stream; charset=utf-8";
            context.Response.Headers.CacheControl = "no-cache, no-transform";
            context.Response.Headers["X-Accel-Buffering"] = "no";
            await context.Response.StartAsync(context.RequestAborted);
            await context.Response.Body.FlushAsync(context.RequestAborted);
            await foreach (RelayEvent item in feed.Subscribe(context.RequestAborted))
            {
                string json = item.Event.ToJson(JsonMode.Full) ?? "{}";
                string method = item.Event.Method.Replace('\r', '_').Replace('\n', '_');
                string data = json.Replace("\r", string.Empty, StringComparison.Ordinal)
                    .Replace("\n", "\ndata: ", StringComparison.Ordinal);
                await context.Response.WriteAsync($"id: {item.Sequence}\nevent: {method}\ndata: {data}\n\n", context.RequestAborted);
                await context.Response.Body.FlushAsync(context.RequestAborted);
            }
        });

        app.MapGet("/vrchat/events", (HttpContext context, LiveEventFeed feed) =>
        {
            if (!feed.IsIpAllowed(context.Connection.RemoteIpAddress))
                return Results.StatusCode(StatusCodes.Status403Forbidden);
            byte[] payload = feed.GetSnapshotPayload();
            context.Response.Headers.CacheControl = "no-store";
            context.Response.Headers["X-Content-Type-Options"] = "nosniff";
            return Results.Bytes(payload, "application/json; charset=utf-8");
        });
    }
}
