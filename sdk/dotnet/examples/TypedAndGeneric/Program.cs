using Ttl.Live;

await using var client = new TikTokLiveClient(new TikTokLiveOptions
{
    UniqueId = args.FirstOrDefault() ?? throw new ArgumentException("Pass a creator unique ID."),
    AutoReconnect = true
});

client.EventReceived += (_, e) =>
{
    TtlEvent value = e.Event;
    Console.WriteLine($"generic {value.Method} #{value.MessageId} support={value.Support}");
    if (value.Root.Find("common")?.ObjectValue is TtlObject common)
        Console.WriteLine($"create time: {common.Find("create_time")?.Value}");
};
client.ChatReceived += (_, e) => Console.WriteLine($"chat {e.User.UniqueId}: {e.Comment}");
client.GiftReceived += (_, e) => Console.WriteLine($"gift {e.GiftName} x{e.RepeatCount}");
client.LikeReceived += (_, e) => Console.WriteLine($"likes {e.Count} (total {e.Total})");
client.MemberReceived += (_, e) => Console.WriteLine($"member action={e.Action} count={e.MemberCount}");
client.SocialReceived += (_, e) => Console.WriteLine($"social action={e.Action} follows={e.FollowCount}");
client.RoomUserReceived += (_, e) => Console.WriteLine($"viewers={e.Total} popularity={e.Popularity}");

await client.ConnectAsync();
await Task.Delay(Timeout.InfiniteTimeSpan);
