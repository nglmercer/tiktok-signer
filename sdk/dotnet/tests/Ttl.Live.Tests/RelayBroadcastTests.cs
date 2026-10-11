using System.Net;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Hosting.Server;
using Microsoft.AspNetCore.Hosting.Server.Features;
using Microsoft.Extensions.DependencyInjection;
using Ttl.Live;

namespace Ttl.Live.Tests;

public sealed class RelayBroadcastTests
{
    private static string Fixture(string name) => Path.Combine(AppContext.BaseDirectory, "fixtures", "events", name);

    [Fact]
    public async Task HttpSseBroadcastsToEverySubscriberAndSnapshotIsConsistent()
    {
        var builder = WebApplication.CreateBuilder();
        using var listener = new System.Net.Sockets.TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        int port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();
        builder.WebHost.UseUrls($"http://127.0.0.1:{port}");
        var feed = new LiveEventFeed(subscriberQueueCapacity: 8, historyCapacity: 16, maxPayloadBytes: 4096);
        builder.Services.AddSingleton(feed);
        await using WebApplication app = builder.Build();
        RelayHttpEndpoints.Map(app);
        await app.StartAsync();

        using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        try
        {
            string address = app.Services.GetRequiredService<IServer>()
                .Features.Get<IServerAddressesFeature>()!.Addresses.Single();
            using var clientA = new HttpClient { BaseAddress = new Uri(address) };
            using var clientB = new HttpClient { BaseAddress = new Uri(address) };
            using HttpResponseMessage responseA = await clientA.GetAsync("/events",
                HttpCompletionOption.ResponseHeadersRead, cancellation.Token);
            using HttpResponseMessage responseB = await clientB.GetAsync("/events",
                HttpCompletionOption.ResponseHeadersRead, cancellation.Token);
            Assert.Equal("text/event-stream", responseA.Content.Headers.ContentType?.MediaType);
            Assert.Equal("text/event-stream", responseB.Content.Headers.ContentType?.MediaType);
            await WaitUntilAsync(() => feed.SubscriberCount == 2);

            TtlEvent chat = TikTokEventDecoder.Decode("WebcastChatMessage", 1, false,
                File.ReadAllBytes(Fixture("chat.pb")));
            feed.Publish(chat);
            using var readerA = new StreamReader(await responseA.Content.ReadAsStreamAsync(cancellation.Token));
            using var readerB = new StreamReader(await responseB.Content.ReadAsStreamAsync(cancellation.Token));
            string[] eventA = await ReadSseEventAsync(readerA, cancellation.Token);
            string[] eventB = await ReadSseEventAsync(readerB, cancellation.Token);
            Assert.Contains("id: 1", eventA);
            Assert.Contains("id: 1", eventB);
            Assert.Contains("event: WebcastChatMessage", eventA);
            Assert.Contains(eventA, line => line.StartsWith("data: ", StringComparison.Ordinal));

            TtlEvent gift = TikTokEventDecoder.Decode("WebcastGiftMessage", 2, false,
                File.ReadAllBytes(Fixture("gift.pb")));
            feed.Publish(gift);
            using HttpResponseMessage snapshot = await clientA.GetAsync("/vrchat/events", cancellation.Token);
            Assert.Equal(HttpStatusCode.OK, snapshot.StatusCode);
            Assert.Equal("no-store", snapshot.Headers.CacheControl?.ToString());
            using JsonDocument document = JsonDocument.Parse(await snapshot.Content.ReadAsByteArrayAsync(cancellation.Token));
            JsonElement root = document.RootElement;
            Assert.Equal(2, root.GetProperty("sequence").GetInt64());
            Assert.Equal("1", root.GetProperty("chat")[0].GetProperty("id").GetString());
            Assert.Equal("2", root.GetProperty("gifts")[0].GetProperty("id").GetString());
            Assert.False(string.IsNullOrWhiteSpace(root.GetProperty("gifts")[0].GetProperty("giftId").GetString()));
        }
        finally
        {
            cancellation.Cancel();
            await app.StopAsync();
            feed.Dispose();
        }
        await WaitUntilAsync(() => feed.SubscriberCount == 0);
    }

    [Fact]
    public async Task SlowSseSubscriberDropsOldestWithoutBlockingAndIsRemovedOnCancellation()
    {
        using var feed = new LiveEventFeed(subscriberQueueCapacity: 2, historyCapacity: 4);
        using var cancellation = new CancellationTokenSource();
        await using IAsyncEnumerator<RelayEvent> reader = feed.Subscribe(cancellation.Token).GetAsyncEnumerator();
        Task<bool> firstRead = reader.MoveNextAsync().AsTask();
        await WaitUntilAsync(() => feed.SubscriberCount == 1);
        TtlEvent chat = TikTokEventDecoder.Decode("WebcastChatMessage", 1, false,
            File.ReadAllBytes(Fixture("chat.pb")));
        feed.Publish(chat);
        Assert.True(await firstRead.WaitAsync(TimeSpan.FromSeconds(2)));
        Assert.Equal(1, reader.Current.Sequence);

        for (int i = 0; i < 5; i++) feed.Publish(chat);

        Assert.True(await reader.MoveNextAsync());
        Assert.Equal(5, reader.Current.Sequence);
        Assert.True(await reader.MoveNextAsync());
        Assert.Equal(6, reader.Current.Sequence);
        Assert.Equal(3, feed.DroppedSubscriberEvents);

        cancellation.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(async () => await reader.MoveNextAsync());
        await WaitUntilAsync(() => feed.SubscriberCount == 0);
    }

    [Fact]
    public async Task ConcurrentSnapshotReadersObserveOrderedDeduplicableEventIds()
    {
        using var feed = new LiveEventFeed(historyCapacity: 64, maxPayloadBytes: 32_768);
        TtlEvent chat = TikTokEventDecoder.Decode("WebcastChatMessage", 1, false,
            File.ReadAllBytes(Fixture("chat.pb")));
        Task[] publishers = Enumerable.Range(0, 4).Select(_ => Task.Run(() =>
        {
            for (int i = 0; i < 100; i++) feed.Publish(chat);
        })).ToArray();
        Task readers = Task.Run(async () =>
        {
            long previousSequence = 0;
            while (publishers.Any(publisher => !publisher.IsCompleted))
            {
                using JsonDocument snapshot = JsonDocument.Parse(feed.GetSnapshotPayload());
                JsonElement root = snapshot.RootElement;
                long sequence = root.GetProperty("sequence").GetInt64();
                Assert.True(sequence >= previousSequence);
                previousSequence = sequence;
                long[] ids = root.GetProperty("chat").EnumerateArray()
                    .Select(item => long.Parse(item.GetProperty("id").GetString()!, System.Globalization.CultureInfo.InvariantCulture))
                    .ToArray();
                Assert.True(ids.Zip(ids.Skip(1), (left, right) => left < right).All(isOrdered => isOrdered));
                Assert.All(ids, id => Assert.InRange(id, 1, sequence));
                Assert.True(ids.Length <= 64);
                await Task.Yield();
            }
        });

        await Task.WhenAll(publishers);
        await readers.WaitAsync(TimeSpan.FromSeconds(5));
        using JsonDocument final = JsonDocument.Parse(feed.GetSnapshotPayload());
        Assert.Equal(400, final.RootElement.GetProperty("sequence").GetInt64());
        Assert.Equal(64, final.RootElement.GetProperty("chat").GetArrayLength());
    }

    [Fact]
    public void SnapshotBoundsHistoryPayloadEscapesUntrustedTextAndSupportsIpAllowlisting()
    {
        var feed = new LiveEventFeed(historyCapacity: 8, maxPayloadBytes: 1024,
            allowedIps: ["192.0.2.10"]);
        TtlEvent chat = ChatWithComment("<script>alert('x')</script>");
        feed.Publish(chat);
        byte[] payload = feed.GetSnapshotPayload();
        Assert.True(payload.Length <= 1024);
        string json = Encoding.UTF8.GetString(payload);
        Assert.DoesNotContain("<script>", json, StringComparison.Ordinal);
        using JsonDocument parsed = JsonDocument.Parse(payload);
        Assert.Equal("<script>alert('x')</script>", parsed.RootElement.GetProperty("chat")[0].GetProperty("text").GetString());
        Assert.True(feed.IsIpAllowed(IPAddress.Parse("192.0.2.10")));
        Assert.False(feed.IsIpAllowed(IPAddress.Loopback));

        var limited = new LiveEventFeed(historyCapacity: 8, maxPayloadBytes: 128);
        limited.Publish(ChatWithComment(new string('x', 512)));
        Assert.True(limited.GetSnapshotPayload().Length <= 128);
        Assert.Equal(1, limited.DroppedSnapshotEvents);
    }

    private static TtlEvent ChatWithComment(string comment)
    {
        byte[] text = Encoding.UTF8.GetBytes(comment);
        var payload = new List<byte> { 0x1a };
        WriteVarint(payload, (ulong)text.Length);
        payload.AddRange(text);
        return TikTokEventDecoder.Decode("WebcastChatMessage", 3, false, payload.ToArray());
    }

    private static void WriteVarint(List<byte> bytes, ulong value)
    {
        while (value >= 0x80)
        {
            bytes.Add((byte)(value | 0x80));
            value >>= 7;
        }
        bytes.Add((byte)value);
    }

    private static async Task<string[]> ReadSseEventAsync(StreamReader reader, CancellationToken cancellationToken)
    {
        var lines = new List<string>();
        while (true)
        {
            string? line = await reader.ReadLineAsync(cancellationToken);
            if (line is null || line.Length == 0) return lines.ToArray();
            lines.Add(line);
        }
    }

    private static async Task WaitUntilAsync(Func<bool> condition)
    {
        DateTime deadline = DateTime.UtcNow.AddSeconds(5);
        while (!condition() && DateTime.UtcNow < deadline) await Task.Delay(10);
        Assert.True(condition(), "Timed out waiting for relay subscribers to close.");
    }
}
