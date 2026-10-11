using System.Net;
using System.Text.Json;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;

namespace Ttl.Live.Tests;

public sealed class InteractiveRelayTests
{
    [Theory]
    [InlineData(" @creator.name ", "creator.name")]
    [InlineData("viewer_123", "viewer_123")]
    public void AcceptsCreatorNames(string input, string expected) =>
        Assert.Equal(expected, InteractiveLiveSession.NormalizeCreator(input));

    [Theory]
    [InlineData("https://tiktok.com/@creator")]
    [InlineData("../creator")]
    [InlineData("creator?cookie=x")]
    [InlineData("")]
    [InlineData("abcdefghijklmnopqrstuvwxy")]
    public void RejectsUrlsAndInvalidNames(string input) =>
        Assert.Throws<ArgumentException>(() => InteractiveLiveSession.NormalizeCreator(input));

    [Fact]
    public void LocalControlsRejectRemoteHostsAndCrossOriginRequests()
    {
        var context = new DefaultHttpContext();
        context.Request.Scheme = "http";
        context.Request.Host = new HostString("localhost", 15050);
        context.Connection.RemoteIpAddress = IPAddress.Loopback;
        Assert.True(OperatorHttpEndpoints.IsLocalOperator(context));
        context.Request.Headers.Origin = "https://evil.example";
        Assert.False(OperatorHttpEndpoints.IsLocalOperator(context));
        context.Request.Headers.Origin = "http://localhost:15050";
        Assert.True(OperatorHttpEndpoints.IsLocalOperator(context));
        context.Request.Host = new HostString("evil.example", 15050);
        context.Request.Headers.Remove("Origin");
        Assert.False(OperatorHttpEndpoints.IsLocalOperator(context));
        context.Request.Host = new HostString("localhost", 15050);
        context.Connection.RemoteIpAddress = IPAddress.Parse("192.0.2.1");
        Assert.False(OperatorHttpEndpoints.IsLocalOperator(context));
    }

    [Fact]
    public async Task FixtureModeUsesNativeDecodingAndKeepsMonotonicIdsAcrossSessions()
    {
        using var feed = new LiveEventFeed();
        await using var session = new InteractiveLiveSession(feed, NullLogger<InteractiveLiveSession>.Instance);
        string directory = Path.Combine(AppContext.BaseDirectory, "fixtures", "events");
        await session.LoadFixturesAsync(directory);
        Assert.True(session.Status.Fixture);
        Assert.Equal("Fixture", session.Status.State);
        using var first = JsonDocument.Parse(JsonSerializer.Serialize(session.Snapshot()));
        Assert.Single(first.RootElement.GetProperty("events").GetProperty("chat").EnumerateArray());
        Assert.Single(first.RootElement.GetProperty("events").GetProperty("gifts").EnumerateArray());
        string streamId = first.RootElement.GetProperty("streamId").GetString()!;
        await Task.WhenAll(session.LoadFixturesAsync(directory), session.LoadFixturesAsync(directory));
        using var last = JsonDocument.Parse(JsonSerializer.Serialize(session.Snapshot()));
        Assert.Equal(streamId, last.RootElement.GetProperty("streamId").GetString());
        Assert.Equal(6, last.RootElement.GetProperty("events").GetProperty("sequence").GetInt64());
        Assert.Equal("5", last.RootElement.GetProperty("events").GetProperty("chat")[0].GetProperty("id").GetString());
        await session.DisconnectAsync();
        Assert.Equal("Disconnected", session.Status.State);
        using var empty = JsonDocument.Parse(feed.GetSnapshotPayload());
        Assert.Empty(empty.RootElement.GetProperty("chat").EnumerateArray());
        await session.DisposeAsync();
        await session.DisposeAsync();
    }
}
