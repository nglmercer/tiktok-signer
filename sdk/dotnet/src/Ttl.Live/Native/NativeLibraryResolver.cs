using System.Reflection;
using System.Runtime.InteropServices;

namespace Ttl.Live.Native;

internal static class NativeLibraryResolver
{
    private const string ImportName = "ttl_live";

    internal static void Register(Assembly assembly)
    {
        try
        {
            NativeLibrary.SetDllImportResolver(assembly, Resolve);
        }
        catch (Exception error) when (error is InvalidOperationException or ArgumentException)
        {
            // The resolver is registered once by NativeMethods' type initializer.
        }
    }

    private static nint Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (!string.Equals(libraryName, ImportName, StringComparison.Ordinal))
            return 0;

        string? explicitPath = Environment.GetEnvironmentVariable("TTL_LIVE_NATIVE_LIBRARY");
        if (!string.IsNullOrWhiteSpace(explicitPath))
        {
            if (!NativeLibrary.TryLoad(explicitPath, out nint explicitHandle))
                throw new DllNotFoundException($"TTL_LIVE_NATIVE_LIBRARY points to an unavailable library: {explicitPath}");
            return explicitHandle;
        }

        string mappedName = OperatingSystem.IsWindows()
            ? "ttl_live.dll"
            : OperatingSystem.IsMacOS() ? "libttl_live.dylib" : "libttl_live.so";
        string rid = RuntimeInformation.RuntimeIdentifier;
        var roots = new HashSet<string>(StringComparer.Ordinal)
        {
            AppContext.BaseDirectory,
            Path.GetDirectoryName(assembly.Location) ?? AppContext.BaseDirectory
        };

        foreach (string root in roots)
        {
            foreach (string candidate in new[]
                     {
                         Path.Combine(root, mappedName),
                         Path.Combine(root, "runtimes", rid, "native", mappedName)
                     })
            {
                if (NativeLibrary.TryLoad(candidate, out nint handle))
                    return handle;
            }
        }

        // Let the normal runtime resolver apply its standard search rules and produce
        // the platform-specific diagnostics if no packaged or explicit path matched.
        return 0;
    }
}
