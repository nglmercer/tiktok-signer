using Ttl.Live;

Console.WriteLine($"schema fingerprint: {TikTokSchemaCatalog.Fingerprint}");
foreach (TtlEventSchemaDescriptor schema in TikTokSchemaCatalog.Events)
    Console.WriteLine($"{schema.Method} -> {schema.FullName} ({schema.Fields.Count} fields)");

string creator = args.FirstOrDefault() ?? throw new ArgumentException("Pass a creator ID to inspect live traffic.");
await using var client = new TikTokLiveClient(new TikTokLiveOptions { UniqueId = creator, AutoReconnect = true });
client.EventReceived += (_, args) =>
{
    TtlEvent value = args.Event;
    Console.WriteLine($"{value.Method} ({value.Support}) schema={value.SchemaName ?? "unknown"} bytes={value.RawPayload.Length}");
    Console.WriteLine(value.ToJson(JsonMode.Full));
    foreach (TtlField field in value.Root.Fields)
        Console.WriteLine($"wire field {field.Number}: {field.Name ?? "<unknown>"} = {field.Value}");
};
await client.ConnectAsync();
await Task.Delay(Timeout.InfiniteTimeSpan);
