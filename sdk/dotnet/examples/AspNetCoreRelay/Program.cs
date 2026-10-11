using Ttl.Live;

var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton(_ => LiveEventFeed.FromEnvironment());
builder.Services.AddHostedService<LiveRelayService>();
var app = builder.Build();
RelayHttpEndpoints.Map(app);

await app.RunAsync();

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

        try
        {
            foreach (string creator in creators)
            {
                var client = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = creator, AutoReconnect = true });
                client.EventReceived += (_, args) => feed.Publish(args.Event);
                client.ErrorReceived += (_, args) => logger.LogError("{Creator}: {Error}", creator, args.Error.Message);
                _clients.Add(client);
            }

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
