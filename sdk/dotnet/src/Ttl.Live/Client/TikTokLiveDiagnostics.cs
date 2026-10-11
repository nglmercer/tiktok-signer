namespace Ttl.Live;

/// <summary>A point-in-time snapshot of callback dispatcher health for one client.</summary>
public sealed record TikTokLiveDiagnostics(
    int QueueDepth,
    int QueueCapacity,
    long QueuedBytes,
    long QueueByteLimit,
    long DroppedEvents,
    long DroppedWorkItems,
    long ProcessingFailures);
