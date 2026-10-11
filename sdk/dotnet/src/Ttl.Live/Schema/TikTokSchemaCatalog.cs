using System.Collections.ObjectModel;
using Ttl.Live.Native;

namespace Ttl.Live;

public sealed record TtlSchemaFieldDescriptor(
    uint Number,
    string Name,
    string JsonName,
    ProtoType Type,
    byte WireType,
    FieldCardinality Cardinality,
    bool IsMap,
    bool IsDeprecated,
    string? OneofName,
    int OneofIndex,
    string? MessageName,
    string? EnumType);

public sealed record TtlEventSchemaDescriptor(
    string Method,
    string FullName,
    uint MethodId,
    IReadOnlyList<TtlSchemaFieldDescriptor> Fields);

/// <summary>Immutable snapshot of every Webcast schema in the native descriptor catalog.</summary>
public static class TikTokSchemaCatalog
{
    private static readonly Lazy<IReadOnlyList<TtlEventSchemaDescriptor>> Catalog = new(Load,
        LazyThreadSafetyMode.ExecutionAndPublication);

    public static string Fingerprint
    {
        get
        {
            NativeRuntime.EnsureLoaded();
            return NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_fingerprint()) ?? string.Empty;
        }
    }

    public static IReadOnlyList<TtlEventSchemaDescriptor> Events => Catalog.Value;

    public static TtlEventSchemaDescriptor? Find(string method)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(method);
        NativeRuntime.EnsureLoaded();
        using var utf8 = new Utf8Input(method);
        nint schema = NativeMethods.ttl_schema_event_find(utf8.Pointer, utf8.Length);
        if (schema == 0) return null;
        string canonicalMethod = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_event_schema_method(schema)) ?? method;
        return Events.FirstOrDefault(item => string.Equals(item.Method, canonicalMethod, StringComparison.Ordinal));
    }

    private static IReadOnlyList<TtlEventSchemaDescriptor> Load()
    {
        NativeRuntime.EnsureLoaded();
        nuint count = NativeMethods.ttl_schema_event_count();
        if (count > int.MaxValue) throw new InvalidDataException("Native schema catalog is too large for managed memory.");
        var result = new TtlEventSchemaDescriptor[(int)count];
        for (nuint index = 0; index < count; index++)
        {
            nint schema = NativeMethods.ttl_schema_event_at(index);
            if (schema == 0) throw new InvalidDataException($"Native schema descriptor {index} is null.");
            string method = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_event_schema_method(schema)) ?? string.Empty;
            string fullName = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_event_schema_full_name(schema)) ?? string.Empty;
            nuint fieldCount = NativeMethods.ttl_event_schema_field_count(schema);
            if (fieldCount > 4096) throw new InvalidDataException($"Schema {fullName} has too many fields.");
            var fields = new TtlSchemaFieldDescriptor[(int)fieldCount];
            for (nuint fieldIndex = 0; fieldIndex < fieldCount; fieldIndex++)
            {
                nint field = NativeMethods.ttl_event_schema_field_at(schema, fieldIndex);
                if (field == 0) throw new InvalidDataException($"Schema {fullName} field {fieldIndex} is null.");
                fields[(int)fieldIndex] = new TtlSchemaFieldDescriptor(
                    NativeMethods.ttl_schema_field_number(field),
                    NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_field_name(field)) ?? string.Empty,
                    NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_field_json_name(field)) ?? string.Empty,
                    (ProtoType)NativeMethods.ttl_schema_field_proto_type(field),
                    NativeMethods.ttl_schema_field_wire_type(field),
                    (FieldCardinality)NativeMethods.ttl_schema_field_cardinality(field),
                    NativeMethods.ttl_schema_field_is_map(field),
                    NativeMethods.ttl_schema_field_is_deprecated(field),
                    NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_field_oneof_name(field)),
                    NativeMethods.ttl_schema_field_oneof_index(field),
                    NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_field_message_name(field)),
                    NativeRuntime.ReadNullTerminated(NativeMethods.ttl_schema_field_enum_type(field)));
            }
            result[(int)index] = new TtlEventSchemaDescriptor(method, fullName,
                NativeMethods.ttl_event_schema_type(schema), Array.AsReadOnly(fields));
        }
        return new ReadOnlyCollection<TtlEventSchemaDescriptor>(result);
    }
}
