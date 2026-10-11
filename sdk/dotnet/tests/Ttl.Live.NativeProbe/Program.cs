using Ttl.Live;

string expected = args.FirstOrDefault() ?? throw new ArgumentException("Expected missing or mismatch.");
try
{
    _ = TikTokLiveClient.NativeLibrary;
    Console.Error.WriteLine("Expected native library loading to fail, but it succeeded.");
    return 1;
}
catch (NativeLibraryException error)
{
    if (!error.Message.Contains(expected, StringComparison.OrdinalIgnoreCase))
    {
        Console.Error.WriteLine(error.Message);
        return 2;
    }
    Console.WriteLine(error.Message);
    return 0;
}
