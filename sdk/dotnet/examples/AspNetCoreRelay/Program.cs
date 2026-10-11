using System.Threading.Channels;
using Ttl.Live;

var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton<LiveEventFeed>();
builder.Services.AddHostedService<LiveRelayService>();
var app = builder.Build();

app.MapGet("/health", () => Results.Ok(new { status = "running" }));
app.MapGet("/events", async (HttpContext context, LiveEventFeed feed) =>
{
    context.Response.ContentType = "text/event-stream";
    context.Response.Headers.CacheControl = "no-cache";
    await foreach (TtlEvent item in feed.ReadAll(context.RequestAborted))
    {
        string json = item.ToJson(JsonMode.Full) ?? "{}";
        await context.Response.WriteAsync($"event: {item.Method}\ndata: {json}\n\n", context.RequestAborted);
        await context.Response.Body.FlushAsync(context.RequestAborted);
    }
});

await app.RunAsync();

sealed class LiveEventFeed
{
    private readonly Channel<TtlEvent> _events = Channel.CreateUnbounded<TtlEvent>();
    internal void Publish(TtlEvent item) => _events.Writer.TryWrite(item);
    internal IAsyncEnumerable<TtlEvent> ReadAll(CancellationToken cancellationToken) =>
        _events.Reader.ReadAllAsync(cancellationToken);
}

sealed class LiveRelayService(ILogger<LiveRelayService> logger, LiveEventFeed feed) : BackgroundService
{
    private readonly List<TikTokLiveClient> _clients = [];

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        string[] creators = (Environment.GetEnvironmentVariable("LIVE_CREATORS") ?? string.Empty)
            .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (creators.Length == 0)
        {
            logger.LogWarning("Set LIVE_CREATORS to a comma-separated list of TikTok creator IDs.");
            await Task.Delay(Timeout.InfiniteTimeSpan, stoppingToken);
            return;
        }

        foreach (string creator in creators)
        {
            var client = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = creator, AutoReconnect = true });
            client.EventReceived += (_, args) => feed.Publish(args.Event);
            client.ErrorReceived += (_, args) => logger.LogError("{Creator}: {Error}", creator, args.Error.Message);
            _clients.Add(client);
        }

        try
        {
            await Task.WhenAll(_clients.Select(client => client.ConnectAsync(stoppingToken)));
            await Task.Delay(Timeout.InfiniteTimeSpan, stoppingToken);
        }
        catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested) { }
        catch (Exception error) { logger.LogError(error, "A LIVE connection failed."); }
        finally
        {
            await Task.WhenAll(_clients.Select(client => client.DisposeAsync().AsTask()));
            _clients.Clear();
        }
    }
}
