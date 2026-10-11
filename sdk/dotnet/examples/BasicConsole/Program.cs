using Ttl.Live;

string creator = args.Length > 0 ? args[0] : throw new ArgumentException("Pass a creator unique ID.");
using var stop = new CancellationTokenSource();
Console.CancelKeyPress += (_, eventArgs) => { eventArgs.Cancel = true; stop.Cancel(); };

await using var client = new TikTokLiveClient(new TikTokLiveOptions
{
    UniqueId = creator,
    AutoReconnect = true
});
client.StateChanged += (_, e) => Console.WriteLine($"state={e.State} {e.Detail}");
client.EventReceived += (_, e) => Console.WriteLine(e.Event.Method);
client.ChatReceived += (_, e) => Console.WriteLine($"{e.User.Nickname}: {e.Comment}");
client.GiftReceived += (_, e) => Console.WriteLine($"{e.User.Nickname} sent {e.GiftName}");
client.ErrorReceived += (_, e) => Console.Error.WriteLine($"{e.Error.NativeCode}: {e.Error.Message}");

await client.ConnectAsync(stop.Token);
try { await Task.Delay(Timeout.InfiniteTimeSpan, stop.Token); }
catch (OperationCanceledException) { }
