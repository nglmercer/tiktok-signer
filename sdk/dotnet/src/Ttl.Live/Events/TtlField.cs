using System.Collections.ObjectModel;

namespace Ttl.Live;

public enum EventSupport { Normalized = 1, Schema = 2, Raw = 3 }
public enum JsonMode { Normalized = 0, Schema = 1, Full = 2, Compact = 3, Raw = 4 }
public enum TtlValueType { Varint = 0, Fixed32 = 1, Fixed64 = 2, String = 3, Bytes = 4, Message = 5, Truncated = 6 }
public enum ProtoType
{
    Double = 0, Float = 1, Int64 = 2, UInt64 = 3, SInt64 = 4, Fixed64 = 5,
    SFixed64 = 6, Int32 = 7, UInt32 = 8, SInt32 = 9, Fixed32 = 10,
    SFixed32 = 11, Bool = 12, String = 13, Bytes = 14, Enum = 15,
    Message = 16, Unknown = 17
}
public enum FieldCardinality { Optional = 0, Repeated = 1 }

/// <summary>A copied field occurrence. Repeated occurrences and unknown wire fields remain ordered.</summary>
public sealed class TtlField
{
    private readonly IReadOnlyList<TtlMapEntry> _mapEntries;
    public uint Number { get; }
    public string? Name { get; }
    public string? JsonName { get; }
    public bool IsNamed { get; }
    public TtlValueType ValueType { get; }
    public byte WireType { get; }
    public ProtoType ProtoType { get; }
    public FieldCardinality Cardinality { get; }
    public bool IsMap { get; }
    public string? OneofName { get; }
    public int OneofIndex { get; }
    public object? Value { get; }
    public TtlObject? ObjectValue { get; }
    public string? EnumType { get; }
    public string? EnumName { get; }
    public IReadOnlyList<TtlMapEntry> MapEntries => _mapEntries;
    public bool IsUnknown => !IsNamed || ProtoType == ProtoType.Unknown;

    internal TtlField(uint number, string? name, string? jsonName, bool isNamed,
        TtlValueType valueType, byte wireType, ProtoType protoType,
        FieldCardinality cardinality, bool isMap, string? oneofName, int oneofIndex,
        object? value, TtlObject? objectValue, string? enumType, string? enumName,
        IReadOnlyList<TtlMapEntry>? mapEntries)
    {
        Number = number; Name = name; JsonName = jsonName; IsNamed = isNamed;
        ValueType = valueType; WireType = wireType; ProtoType = protoType;
        Cardinality = cardinality; IsMap = isMap; OneofName = oneofName;
        OneofIndex = oneofIndex; Value = value; ObjectValue = objectValue;
        EnumType = enumType; EnumName = enumName;
        _mapEntries = mapEntries ?? Array.Empty<TtlMapEntry>();
    }

    public bool TryGet<T>(out T? value)
    {
        if (Value is T typed) { value = typed; return true; }
        value = default;
        return false;
    }

    internal TtlField Clone()
    {
        object? copiedValue = Value is ReadOnlyMemory<byte> bytes
            ? new ReadOnlyMemory<byte>(bytes.ToArray())
            : Value;
        TtlObject? copiedObject = ObjectValue?.Clone();
        var copiedMaps = _mapEntries.Select(entry => new TtlMapEntry(entry.Key?.Clone(), entry.Value?.Clone())).ToArray();
        return new TtlField(Number, Name, JsonName, IsNamed, ValueType, WireType,
            ProtoType, Cardinality, IsMap, OneofName, OneofIndex, copiedValue,
            copiedObject, EnumType, EnumName, copiedMaps);
    }
}

public sealed record TtlMapEntry(TtlField? Key, TtlField? Value);

/// <summary>Copied protobuf message object in native wire order.</summary>
public sealed class TtlObject
{
    private readonly IReadOnlyList<TtlField> _fields;
    private readonly IReadOnlyDictionary<string, TtlField> _activeOneofs;
    public IReadOnlyList<TtlField> Fields => _fields;
    public bool IsTruncated { get; }

    internal TtlObject(IEnumerable<TtlField> fields, bool isTruncated,
        IReadOnlyDictionary<string, TtlField>? activeOneofs = null)
    {
        _fields = new ReadOnlyCollection<TtlField>(fields.ToArray());
        IsTruncated = isTruncated;
        _activeOneofs = activeOneofs ?? new Dictionary<string, TtlField>();
    }

    /// <summary>Returns the first matching proto or JSON name, matching the C API contract.</summary>
    public TtlField? Find(string name) => _fields.FirstOrDefault(f =>
        string.Equals(f.Name, name, StringComparison.Ordinal) ||
        string.Equals(f.JsonName, name, StringComparison.Ordinal));

    public IReadOnlyList<TtlField> FindAll(string name) => new ReadOnlyCollection<TtlField>(
        _fields.Where(f => string.Equals(f.Name, name, StringComparison.Ordinal) ||
                           string.Equals(f.JsonName, name, StringComparison.Ordinal)).ToArray());

    public TtlField? ActiveOneof(string name) => _activeOneofs.GetValueOrDefault(name);
    public IReadOnlyList<TtlMapEntry> MapEntries(string name) => Find(name)?.MapEntries ?? Array.Empty<TtlMapEntry>();

    internal TtlObject Clone()
    {
        TtlField[] source = _fields.ToArray();
        TtlField[] fields = source.Select(field => field.Clone()).ToArray();
        var active = new Dictionary<string, TtlField>(StringComparer.Ordinal);
        foreach (var pair in _activeOneofs)
        {
            int index = Array.IndexOf(source, pair.Value);
            if (index >= 0) active[pair.Key] = fields[index];
        }
        return new TtlObject(fields, IsTruncated, active);
    }
}
