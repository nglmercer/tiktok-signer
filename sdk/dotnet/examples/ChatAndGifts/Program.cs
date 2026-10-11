using Ttl.Live;

await using var client = new TikTokLiveClient(new TikTokLiveOptions
{
    UniqueId = args.FirstOrDefault() ?? throw new ArgumentException("Pass a creator unique ID."),
    AutoReconnect = true
});

client.ChatReceived += (_, e) => Console.WriteLine($"{e.User.Nickname} (@{e.User.UniqueId}): {e.Comment}");
client.GiftReceived += (_, e) =>
{
    string burst = e.RepeatEnd ? $"combo finished at {e.ComboCount}" : $"repeat {e.RepeatCount}";
    Console.WriteLine($"{e.User.Nickname} sent {e.GiftName} ({e.DiamondCount} diamonds), {burst}");
};

await client.ConnectAsync();
await Task.Delay(Timeout.InfiniteTimeSpan);
