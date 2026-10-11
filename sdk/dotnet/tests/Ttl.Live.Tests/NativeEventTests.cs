using System.Text.Json;
using Ttl.Live.Native;

namespace Ttl.Live.Tests;

public sealed class NativeEventTests
{
    private static string Fixture(string name) => Path.Combine(AppContext.BaseDirectory, "fixtures", "events", name);

    [Fact]
    public void LibraryAbiAndDescriptorCatalogAreAvailable()
    {
        NativeLibraryInfo library = TikTokLiveClient.NativeLibrary;
        Assert.Equal(1u, library.AbiVersion);
        Assert.False(string.IsNullOrWhiteSpace(library.Version));
        Assert.Equal(64, TikTokSchemaCatalog.Fingerprint.Length);
        Assert.NotEmpty(TikTokSchemaCatalog.Events);
        Assert.Equal(TikTokSchemaCatalog.Events.Count, TikTokSchemaCatalog.Events.Select(e => e.FullName).Distinct().Count());
        Assert.NotNull(TikTokSchemaCatalog.Find("WebcastChatMessage"));
        Assert.Null(TikTokSchemaCatalog.Find("WebcastNoSuchFutureMessage"));
    }

    [Theory]
    [InlineData("WebcastChatMessage", "chat.pb", "chat", typeof(ChatLiveEvent))]
    [InlineData("WebcastGiftMessage", "gift.pb", "gift", typeof(GiftLiveEvent))]
    [InlineData("WebcastLikeMessage", "like.pb", "like", typeof(LikeLiveEvent))]
    [InlineData("WebcastMemberMessage", "member.pb", "member", typeof(MemberLiveEvent))]
    [InlineData("WebcastSocialMessage", "social.pb", "social", typeof(SocialLiveEvent))]
    [InlineData("WebcastRoomUserSeqMessage", "room-user.pb", "room_user", typeof(RoomUserLiveEvent))]
    public void NormalizedEventsKeepTheirTypedAndGenericRepresentations(
        string method, string fixture, string jsonType, Type normalizedType)
    {
        TtlEvent item = TikTokEventDecoder.Decode(method, ulong.MaxValue, isHistory: true,
            File.ReadAllBytes(Fixture(fixture)));
        Assert.Equal(method, item.Method);
        Assert.Equal(ulong.MaxValue, item.MessageId);
        Assert.True(item.IsHistory);
        Assert.Equal(EventSupport.Normalized, item.Support);
        Assert.IsType(normalizedType, item.Normalized);
        Assert.True(item.Root.Fields.Count > 0);
        Assert.False(item.RawPayload.IsEmpty);
        using var normalized = JsonDocument.Parse(item.ToJson(JsonMode.Normalized)!);
        Assert.Equal(jsonType, normalized.RootElement.GetProperty("type").GetString());
        foreach (JsonMode mode in Enum.GetValues<JsonMode>())
        {
            string? json = item.ToJson(mode);
            if (mode == JsonMode.Normalized) Assert.NotNull(json);
            else Assert.False(string.IsNullOrWhiteSpace(json));
            if (json is not null) using (JsonDocument.Parse(json)) { }
        }
        using var full = JsonDocument.Parse(item.ToJson(JsonMode.Full)!);
        Assert.Equal(method, full.RootElement.GetProperty("method").GetString());
        Assert.Equal(ulong.MaxValue.ToString(), full.RootElement.GetProperty("messageId").GetString());
    }

    [Fact]
    public void SchemaOnlyAndUnknownMethodsRetainRawAndSchemaData()
    {
        TtlEvent schema = TikTokEventDecoder.Decode("WebcastGiftPanelUpdateMessage", 7, false,
            File.ReadAllBytes(Fixture("gift-panel-update.pb")));
        Assert.Equal(EventSupport.Schema, schema.Support);
        Assert.Null(schema.Normalized);
        Assert.NotNull(schema.SchemaName);
        Assert.Null(schema.ToJson(JsonMode.Normalized));
        Assert.NotEmpty(schema.Root.Fields);
        using var full = JsonDocument.Parse(schema.ToJson(JsonMode.Full)!);
        Assert.Equal(schema.Method, full.RootElement.GetProperty("method").GetString());

        TtlEvent unknown = TikTokEventDecoder.Decode("WebcastNoSuchFutureMessage", 8, true,
            new byte[] { 8, 42, 18, 3, 0, 1, 2 });
        Assert.Equal(EventSupport.Raw, unknown.Support);
        Assert.Null(unknown.SchemaName);
        Assert.Null(unknown.Normalized);
        Assert.Equal(new byte[] { 8, 42, 18, 3, 0, 1, 2 }, unknown.RawPayload.ToArray());
        Assert.Null(unknown.ToJson(JsonMode.Normalized));
        using var raw = JsonDocument.Parse(unknown.ToJson(JsonMode.Raw)!);
        Assert.Equal("CCoSAwABAg==", raw.RootElement.GetProperty("payload").GetProperty("$bytes").GetString());
    }

    [Fact]
    public void StringViewsUseExplicitLengthsAndPreserveEmbeddedNul()
    {
        // WebcastChatMessage.content is field 3; explicit lengths must retain its embedded NUL.
        byte[] payload = [0x12, 0x00, 0x1a, 0x03, (byte)'a', 0, (byte)'b'];
        TtlEvent item = TikTokEventDecoder.Decode("WebcastChatMessage", 1, false, payload);
        Assert.Equal("a\0b", item.Root.Find("content")?.Value);
        Assert.Equal("a\0b", Assert.IsType<ChatLiveEvent>(item.Normalized).Comment);
        Assert.Contains("a\\u0000b", item.ToJson(JsonMode.Normalized));
    }

    [Fact]
    public void StrictScalarAndNestedMessageGettersCoverEveryProtobufLogicalType()
    {
        ProtoType[] coveredTypes = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields)
            .Where(field => !field.IsMap && field.Cardinality == FieldCardinality.Optional)
            .Select(field => field.Type).Distinct().ToArray();
        Assert.Contains(ProtoType.Int64, coveredTypes);
        Assert.Contains(ProtoType.String, coveredTypes);
        Assert.Contains(ProtoType.Message, coveredTypes);
        foreach (ProtoType type in coveredTypes)
        {
            var candidate = TikTokSchemaCatalog.Events
                .SelectMany(schema => schema.Fields.Where(field => field.Type == type &&
                    field.Cardinality == FieldCardinality.Optional && !field.IsMap)
                    .Select(field => (schema.Method, Field: field)))
                .FirstOrDefault();
            Assert.True(candidate.Field is not null, $"No Webcast schema field uses protobuf type {type}.");
            byte[] payload = PayloadFor(candidate.Field);
            TtlEvent decoded = TikTokEventDecoder.Decode(candidate.Method, 91, false, payload);
            TtlField field = Assert.IsType<TtlField>(decoded.Root.Find(candidate.Field.Name));
            Assert.NotNull(field.Value);
            Assert.Equal(type, field.ProtoType);
            if (type == ProtoType.Message) Assert.NotNull(field.ObjectValue);
            if (type == ProtoType.Enum)
            {
                Assert.IsType<int>(field.Value);
                Assert.NotNull(field.EnumType);
            }
            if (type == ProtoType.String) Assert.Equal("a\0b", field.Value);
            if (type == ProtoType.Bytes) Assert.IsType<ReadOnlyMemory<byte>>(field.Value);
        }

        TtlEvent unknown = TikTokEventDecoder.Decode("WebcastNoSuchFutureMessage", 92, false, [8, 42]);
        Assert.Equal(42UL, Assert.IsType<ulong>(unknown.Root.Fields[0].Value));
        Assert.Equal(1u, unknown.Root.Fields[0].Number);

        using var method = new Utf8Input("WebcastNoSuchFutureMessage");
        using var fixed32 = PinnedInput.From([13, 1, 2, 3, 4]);
        nint nativeEvent = NativeMethods.ttl_event_decode(method.Pointer, method.Length, 0,
            false, fixed32.Pointer, fixed32.Length);
        Assert.NotEqual(0, nativeEvent);
        using var owned = new NativeEventHandle(nativeEvent);
        nint nativeField = NativeMethods.ttl_object_field_at(NativeMethods.ttl_event_root(nativeEvent), 0);
        Assert.False(NativeMethods.ttl_field_get_f32(nativeField, out _));
        Assert.False(NativeMethods.ttl_field_get_f64(nativeField, out _));

        var invalid = Assert.Throws<TikTokLiveException>(() =>
            TikTokEventDecoder.Decode(new string('x', 4097), 0, false, []));
        Assert.Equal((int)NativeResultCode.InvalidArgument, invalid.NativeCode);
        Assert.Contains("invalid pointer", invalid.Message, StringComparison.OrdinalIgnoreCase);

        using var invalidUtf8 = PinnedInput.From([255]);
        using var emptyPayload = PinnedInput.From([]);
        Assert.Equal(0, NativeMethods.ttl_event_decode(invalidUtf8.Pointer, invalidUtf8.Length,
            0, false, emptyPayload.Pointer, emptyPayload.Length));
        TikTokLiveException invalidText = NativeRuntime.LastError("decode invalid UTF-8");
        Assert.Equal((int)NativeResultCode.InvalidUtf8, invalidText.NativeCode);
    }

    [Fact]
    public void BatchAndNativeEventCloneRespectOwnedAndBorrowedLifetimes()
    {
        byte[] payload = File.ReadAllBytes(Fixture("batch.pb"));
        TtlEventBatch batch = TikTokEventDecoder.DecodeBatch(payload);
        Assert.NotEmpty(batch.Events);
        Assert.NotEmpty(batch.Cursor);
        Assert.NotEmpty(batch.InternalExt);

        using var method = new Ttl.Live.Native.Utf8Input("WebcastFutureMessage");
        using var bytes = Ttl.Live.PinnedInput.From(new byte[] { 8, 42 });
        nint eventPointer = Ttl.Live.Native.NativeMethods.ttl_event_decode(method.Pointer, method.Length,
            2, false, bytes.Pointer, bytes.Length);
        Assert.NotEqual(0, eventPointer);
        using var original = new Ttl.Live.Native.NativeEventHandle(eventPointer);
        using var clone = Ttl.Live.Native.NativeEventHandle.Clone(eventPointer);
        TtlEvent clonedValue = Ttl.Live.Native.NativeEventReader.CaptureEvent(clone.DangerousGetHandle());
        Assert.Equal(2UL, clonedValue.MessageId);
        original.Dispose();
        Assert.Equal(new byte[] { 8, 42 }, clonedValue.RawPayload.ToArray());
        original.Dispose();
        clone.Dispose();

        TtlEvent managedClone = clonedValue.Clone();
        Assert.True(System.Runtime.InteropServices.MemoryMarshal.TryGetArray(clonedValue.RawPayload, out var segment));
        segment.Array![segment.Offset] = 99;
        Assert.Equal(new byte[] { 8, 42 }, managedClone.RawPayload.ToArray());
    }

    [Fact]
    public void ManagedCloneRelinksNestedValuesAndCopiesNestedBuffersMapsAndOneofs()
    {
        TtlEvent chat = TikTokEventDecoder.Decode("WebcastChatMessage", 12, false,
            File.ReadAllBytes(Fixture("chat.pb")));
        TtlField originalUser = Assert.IsType<TtlField>(chat.Root.Find("user"));
        Assert.Same(originalUser.ObjectValue, originalUser.Value);
        TtlEvent chatClone = chat.Clone();
        TtlField clonedUser = Assert.IsType<TtlField>(chatClone.Root.Find("user"));
        Assert.NotSame(originalUser.ObjectValue, clonedUser.ObjectValue);
        Assert.Same(clonedUser.ObjectValue, clonedUser.Value);

        TtlSchemaFieldDescriptor bytesDescriptor = TikTokSchemaCatalog.Events
            .SelectMany(schema => schema.Fields)
            .First(field => field.Type == ProtoType.Bytes && !field.IsMap);
        string bytesMethod = TikTokSchemaCatalog.Events
            .First(schema => schema.Fields.Contains(bytesDescriptor)).Method;
        TtlEvent bytesEvent = TikTokEventDecoder.Decode(bytesMethod, 13, false, PayloadFor(bytesDescriptor));
        TtlEvent bytesClone = bytesEvent.Clone();
        TtlField originalBytes = Assert.IsType<TtlField>(bytesEvent.Root.Find(bytesDescriptor.Name));
        TtlField clonedBytes = Assert.IsType<TtlField>(bytesClone.Root.Find(bytesDescriptor.Name));
        Assert.True(System.Runtime.InteropServices.MemoryMarshal.TryGetArray(
            Assert.IsType<ReadOnlyMemory<byte>>(originalBytes.Value), out var originalSegment));
        Assert.True(System.Runtime.InteropServices.MemoryMarshal.TryGetArray(
            Assert.IsType<ReadOnlyMemory<byte>>(clonedBytes.Value), out var clonedSegment));
        Assert.NotSame(originalSegment.Array, clonedSegment.Array);
        originalSegment.Array![originalSegment.Offset] = 77;
        Assert.Equal(new byte[] { 0, 1, 255 }, clonedSegment.Array!.Skip(clonedSegment.Offset).Take(3));

        TtlSchemaFieldDescriptor mapDescriptor = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields)
            .First(field => field.IsMap);
        TtlEventSchemaDescriptor mapSchema = TikTokSchemaCatalog.Events
            .First(schema => schema.Fields.Contains(mapDescriptor));
        TtlEvent mapEvent = TikTokEventDecoder.Decode(mapSchema.Method, 14, false,
            Concat(LengthDelimited(mapDescriptor.Number, []), LengthDelimited(mapDescriptor.Number, [])));
        TtlEvent mapClone = mapEvent.Clone();
        TtlField originalMap = Assert.IsType<TtlField>(mapEvent.Root.Find(mapDescriptor.Name));
        TtlField clonedMap = Assert.IsType<TtlField>(mapClone.Root.Find(mapDescriptor.Name));
        Assert.NotSame(originalMap.MapEntries, clonedMap.MapEntries);
        Assert.Equal(originalMap.MapEntries.Count, clonedMap.MapEntries.Count);
        if (originalMap.MapEntries.Count > 0)
            Assert.NotSame(originalMap.MapEntries[0], clonedMap.MapEntries[0]);

        var oneof = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields
                .Where(field => field.OneofName is not null && field.WireType == 2)
                .Select(field => (schema.Method, Field: field)))
            .FirstOrDefault();
        if (oneof.Field is not null)
        {
            TtlEvent oneofEvent = TikTokEventDecoder.Decode(oneof.Method, 15, false,
                LengthDelimited(oneof.Field.Number, []));
            TtlEvent oneofClone = oneofEvent.Clone();
            TtlField active = Assert.IsType<TtlField>(oneofClone.Root.ActiveOneof(oneof.Field.OneofName!));
            Assert.Contains(active, oneofClone.Root.Fields);
            Assert.NotSame(oneofEvent.Root.ActiveOneof(oneof.Field.OneofName!), active);
        }
    }

    [Fact]
    public void RepeatedEnumMapAndOneofTraversalKeepMetadata()
    {
        var repeated = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields
                .Where(field => field.Cardinality == FieldCardinality.Repeated &&
                                field.Type is ProtoType.Int64 or ProtoType.UInt64 or ProtoType.SInt64))
            .First();
        byte[] packed = [1, 2, 3];
        byte[] repeatedPayload = Concat(LengthDelimited(repeated.Number, packed), Varint(repeated.Number, 4));
        TtlEvent repeatedEvent = TikTokEventDecoder.Decode(repeatedEventMethod(repeated), 1, false, repeatedPayload);
        IReadOnlyList<TtlField> repeatedValues = repeatedEvent.Root.FindAll(repeated.Name);
        Assert.Equal(4, repeatedValues.Count);
        Assert.Equal(new long[] { 1, 2, 3, 4 }, repeatedValues.Select(field => Convert.ToInt64(field.Value)).ToArray());

        TtlEvent enumEvent = TikTokEventDecoder.Decode("WebcastMemberMessage", 2, false, Varint(10, 99999));
        TtlField enumField = Assert.IsType<TtlField>(enumEvent.Root.Find("action"));
        Assert.Equal(99999, enumField.Value);
        Assert.Equal("webcast.im.MemberMessageAction", enumField.EnumType);
        Assert.Null(enumField.EnumName);

        TtlSchemaFieldDescriptor mapDescriptor = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields)
            .First(field => field.IsMap);
        TtlEventSchemaDescriptor mapSchema = TikTokSchemaCatalog.Find(
            TikTokSchemaCatalog.Events.First(schema => schema.Fields.Contains(mapDescriptor)).Method)!;
        TtlEvent mapEvent = TikTokEventDecoder.Decode(mapSchema.Method, 3, false,
            Concat(LengthDelimited(mapDescriptor.Number, []), LengthDelimited(mapDescriptor.Number, [])));
        TtlField mapField = Assert.IsType<TtlField>(mapEvent.Root.Find(mapDescriptor.Name));
        Assert.True(mapField.IsMap);
        Assert.Equal(2, mapField.MapEntries.Count);

        var oneof = TikTokSchemaCatalog.Events.SelectMany(schema => schema.Fields
                .Where(field => field.OneofName is not null && field.WireType == 2)
                .Select(field => (schema.Method, Field: field)))
            .FirstOrDefault();
        if (oneof.Field is not null)
        {
            TtlEvent oneofEvent = TikTokEventDecoder.Decode(oneof.Method, 4, false,
                LengthDelimited(oneof.Field.Number, []));
            Assert.NotNull(oneofEvent.Root.ActiveOneof(oneof.Field.OneofName!));
        }
        else
        {
            TtlEvent oneofEvent = TikTokEventDecoder.Decode("WebcastChatMessage", 4, false, []);
            Assert.Null(oneofEvent.Root.ActiveOneof("no-oneof-present"));
        }
    }

    private static string repeatedEventMethod(TtlSchemaFieldDescriptor field) =>
        TikTokSchemaCatalog.Events.First(schema => schema.Fields.Contains(field)).Method;

    private static byte[] Varint(uint field, ulong value)
    {
        var bytes = new List<byte>();
        WriteVarint(bytes, ((ulong)field << 3));
        WriteVarint(bytes, value);
        return bytes.ToArray();
    }

    private static byte[] PayloadFor(TtlSchemaFieldDescriptor field)
    {
        return field.Type switch
        {
            ProtoType.Double => Fixed64(field.Number, BitConverter.DoubleToUInt64Bits(12.5)),
            ProtoType.Fixed64 or ProtoType.SFixed64 => Fixed64(field.Number, 42),
            ProtoType.Float => Fixed32(field.Number, BitConverter.SingleToUInt32Bits(1.5f)),
            ProtoType.Fixed32 or ProtoType.SFixed32 => Fixed32(field.Number, 42),
            ProtoType.SInt64 => Varint(field.Number, 24),
            ProtoType.SInt32 => Varint(field.Number, 24),
            ProtoType.Bool => Varint(field.Number, 1),
            ProtoType.String => LengthDelimited(field.Number, [(byte)'a', 0, (byte)'b']),
            ProtoType.Bytes => LengthDelimited(field.Number, [0, 1, 255]),
            ProtoType.Message => LengthDelimited(field.Number, []),
            ProtoType.Enum => Varint(field.Number, 99999),
            _ => Varint(field.Number, 42)
        };
    }

    private static byte[] Fixed32(uint field, uint value)
    {
        var bytes = new List<byte>();
        WriteVarint(bytes, ((ulong)field << 3) | 5);
        bytes.AddRange(BitConverter.GetBytes(value));
        return bytes.ToArray();
    }

    private static byte[] Fixed64(uint field, ulong value)
    {
        var bytes = new List<byte>();
        WriteVarint(bytes, ((ulong)field << 3) | 1);
        bytes.AddRange(BitConverter.GetBytes(value));
        return bytes.ToArray();
    }

    private static byte[] LengthDelimited(uint field, byte[] value)
    {
        var bytes = new List<byte>();
        WriteVarint(bytes, ((ulong)field << 3) | 2);
        WriteVarint(bytes, (ulong)value.Length);
        bytes.AddRange(value);
        return bytes.ToArray();
    }

    private static byte[] Concat(params byte[][] pieces) => pieces.SelectMany(piece => piece).ToArray();
    private static void WriteVarint(List<byte> output, ulong value)
    {
        while (value >= 0x80) { output.Add((byte)(value | 0x80)); value >>= 7; }
        output.Add((byte)value);
    }
}
