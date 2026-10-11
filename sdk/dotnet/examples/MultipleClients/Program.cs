using Ttl.Live;

string[] creators = args.Length > 0 ? args : throw new ArgumentException("Pass one or more creator IDs.");
var clients = creators.Select(creator => new TikTokLiveClient(new TikTokLiveOptions
{
    UniqueId = creator,
    AutoReconnect = true
})).ToArray();
try
{
    foreach (var client in clients)
    {
        string creator = client.UniqueId;
        client.EventReceived += (_, e) => Console.WriteLine($"{creator}: {e.Event.Method}");
        client.ErrorReceived += (_, e) => Console.Error.WriteLine($"{creator}: {e.Error.Message}");
    }
    await Task.WhenAll(clients.Select(client => client.ConnectAsync()));
    await Task.Delay(Timeout.InfiniteTimeSpan);
}
finally
{
    await Task.WhenAll(clients.Select(client => client.DisposeAsync().AsTask()));
}
