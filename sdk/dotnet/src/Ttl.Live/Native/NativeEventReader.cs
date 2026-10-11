using System.Collections.ObjectModel;
using System.Text.Json;

namespace Ttl.Live.Native;

internal static class NativeEventReader
{
    private const int MaxNativeDepth = 8;

    internal static TtlEvent CaptureEvent(nint value)
    {
        if (value == 0) throw new InvalidDataException("Native callback supplied a null event.");
        if (!NativeMethods.ttl_event_get_info(value, out NativeEventInfo info))
            throw NativeRuntime.LastError("read event metadata");

        string method = NativeRuntime.ReadUtf8(NativeMethods.ttl_event_method(value), NativeMethods.ttl_event_method_len(value));
        if (!string.Equals(method, NativeRuntime.ReadUtf8(info.Method, info.MethodLen), StringComparison.Ordinal))
            throw new InvalidDataException("Event method views disagree within the native ABI.");
        ulong messageId = NativeMethods.ttl_event_message_id(value);
        bool isHistory = NativeMethods.ttl_event_is_history(value);
        string? schemaName = NativeRuntime.ReadOptionalUtf8(info.SchemaName, info.SchemaNameLen);
        string? eventSchemaName = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_event_schema_name(value));
        if (!string.Equals(schemaName, eventSchemaName, StringComparison.Ordinal))
            throw new InvalidDataException("Event schema name views disagree within the native ABI.");
        byte[] payload = NativeRuntime.CopyBytes(NativeMethods.ttl_event_raw_data(value), NativeMethods.ttl_event_raw_size(value));
        nint nativeRoot = NativeMethods.ttl_event_root(value);
        TtlObject root = CaptureObject(nativeRoot, 0);
        var json = new Dictionary<JsonMode, string?>();
        foreach (JsonMode mode in Enum.GetValues<JsonMode>())
        {
            nint allocated = NativeMethods.ttl_event_to_json(value, (int)mode);
            if (allocated == 0)
            {
                if (mode != JsonMode.Normalized)
                    throw NativeRuntime.LastError($"serialize event as {mode}");
                json[mode] = null;
                continue;
            }
            try { json[mode] = NativeRuntime.ReadNullTerminated(allocated); }
            finally { NativeMethods.ttl_string_free(allocated); }
        }

        uint flags = NativeMethods.ttl_event_flags(value);
        NativeMethods.ttl_event_timestamp(value, out ulong timestamp);
        ulong? maybeTimestamp = info.HasTimestamp != 0 ? timestamp : null;
        int nativeSupport = NativeMethods.ttl_event_support(value);
        var support = Enum.IsDefined(typeof(EventSupport), nativeSupport)
            ? (EventSupport)nativeSupport : EventSupport.Raw;
        bool truncated = NativeMethods.ttl_event_is_truncated(value);
        if (truncated != ((flags & 1) != 0)) throw new InvalidDataException("Event truncation views disagree within the native ABI.");
        var normalized = CaptureNormalized(value, method, nativeSupport, json[JsonMode.Normalized]);
        return new TtlEvent(method, info.MethodId, NativeMethods.ttl_event_type(value),
            messageId, isHistory, support, schemaName, maybeTimestamp,
            flags, payload, root, normalized,
            new ReadOnlyDictionary<JsonMode, string?>(json));
    }

    internal static TtlEventBatch CaptureBatch(nint value)
    {
        if (value == 0) throw new InvalidDataException("Native callback supplied a null batch.");
        nuint count = NativeMethods.ttl_batch_event_count(value);
        if (count > int.MaxValue) throw new InvalidDataException("Native batch has too many events for managed memory.");
        var events = new TtlEvent[(int)count];
        for (nuint i = 0; i < count; i++)
        {
            nint borrowed = NativeMethods.ttl_batch_event_at(value, i);
            if (borrowed == 0) throw new InvalidDataException($"Native batch event {i} is null.");
            events[(int)i] = CaptureEvent(borrowed);
        }
        return new TtlEventBatch(Array.AsReadOnly(events),
            NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_cursor(value)) ?? string.Empty,
            NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_internal_ext(value)) ?? string.Empty,
            NativeRuntime.ReadNullTerminated(NativeMethods.ttl_batch_push_server(value)) ?? string.Empty,
            NativeMethods.ttl_batch_need_ack(value),
            NativeMethods.ttl_batch_heartbeat_duration(value));
    }

    private static TtlObject CaptureObject(nint value, int depth)
    {
        if (value == 0) return new TtlObject(Array.Empty<TtlField>(), true);
        if (depth >= MaxNativeDepth)
            return new TtlObject(Array.Empty<TtlField>(), true);

        nuint nativeCount = NativeMethods.ttl_object_field_count(value);
        if (nativeCount > 4096) throw new InvalidDataException($"Native object has {nativeCount} fields, beyond the ABI bound.");
        int count = (int)nativeCount;
        var nativeFields = new nint[count];
        var names = new string?[count];
        for (int i = 0; i < count; i++)
        {
            nativeFields[i] = NativeMethods.ttl_object_field_at(value, (nuint)i);
            if (nativeFields[i] == 0) throw new InvalidDataException($"Native object field {i} is null.");
            names[i] = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_name(nativeFields[i]));
        }

        // Exercise find_all's count/capacity semantics and preserve each occurrence in wire order.
        foreach (var group in names.Select((name, index) => (name, index))
                     .Where(item => item.name is not null)
                     .GroupBy(item => item.name!, StringComparer.Ordinal))
        {
            using var utf8 = new Utf8Input(group.Key);
            nint first = NativeMethods.ttl_object_find(value, utf8.Pointer, utf8.Length);
            nint expectedFirst = nativeFields[group.Min(item => item.index)];
            if (first != expectedFirst) throw new InvalidDataException("Native find did not return the first matching wire occurrence.");
            nuint total = NativeMethods.ttl_object_find_all(value, utf8.Pointer, utf8.Length, 0, 0);
            if (total > (nuint)count) throw new InvalidDataException("Native find_all returned more fields than the containing object.");
            if (total == 0) continue;
            nint output = System.Runtime.InteropServices.Marshal.AllocHGlobal(checked((int)total * IntPtr.Size));
            try
            {
                nuint written = NativeMethods.ttl_object_find_all(value, utf8.Pointer, utf8.Length, output, total);
                if (written != total) throw new InvalidDataException("Native find_all count changed during an immutable event read.");
                for (int i = 0; i < (int)total; i++)
                {
                    nint found = System.Runtime.InteropServices.Marshal.ReadIntPtr(output, i * IntPtr.Size);
                    if (!nativeFields.Contains(found)) throw new InvalidDataException("Native find_all returned a field outside its parent object.");
                }
            }
            finally { System.Runtime.InteropServices.Marshal.FreeHGlobal(output); }
        }

        var fields = new TtlField[count];
        var activeIndices = new Dictionary<string, int>(StringComparer.Ordinal);
        for (int i = 0; i < count; i++) fields[i] = CaptureField(nativeFields[i], depth);

        foreach (string oneof in fields.Select(f => f.OneofName).Where(n => n is not null).Cast<string>().Distinct(StringComparer.Ordinal))
        {
            using var utf8 = new Utf8Input(oneof);
            nint activeField = NativeMethods.ttl_object_oneof_active(value, utf8.Pointer, utf8.Length);
            if (activeField != 0)
            {
                int index = Array.IndexOf(nativeFields, activeField);
                if (index >= 0) activeIndices[oneof] = index;
            }
        }

        // The native map helpers preserve duplicate keys and map-entry wire order.
        var maps = new Dictionary<string, IReadOnlyList<TtlMapEntry>>(StringComparer.Ordinal);
        for (int i = 0; i < count; i++)
        {
            if (!fields[i].IsMap || fields[i].Name is not string mapName || maps.ContainsKey(mapName)) continue;
            using var utf8 = new Utf8Input(mapName);
            nuint mapCount = NativeMethods.ttl_object_map_size(value, utf8.Pointer, utf8.Length);
            if (mapCount > 65536) throw new InvalidDataException("Native map exceeds the event field bound.");
            var entries = new TtlMapEntry[(int)mapCount];
            for (nuint entry = 0; entry < mapCount; entry++)
            {
                nint key = NativeMethods.ttl_object_map_key_at(value, utf8.Pointer, utf8.Length, entry);
                nint item = NativeMethods.ttl_object_map_value_at(value, utf8.Pointer, utf8.Length, entry);
                entries[(int)entry] = new TtlMapEntry(
                    key == 0 ? null : CaptureField(key, depth),
                    item == 0 ? null : CaptureField(item, depth));
            }
            maps.Add(mapName, Array.AsReadOnly(entries));
        }

        if (maps.Count != 0)
        {
            for (int i = 0; i < count; i++)
            {
                if (fields[i].Name is string name && maps.TryGetValue(name, out var entries) && fields[i].IsMap)
                {
                    // One canonical map view holds every repeated entry. Field occurrences themselves stay intact.
                    fields[i] = WithMapEntries(fields[i], i == Array.FindIndex(fields, f => f.Name == name) ? entries : Array.Empty<TtlMapEntry>());
                }
            }
        }

        var active = activeIndices.ToDictionary(pair => pair.Key, pair => fields[pair.Value], StringComparer.Ordinal);
        return new TtlObject(fields, NativeMethods.ttl_object_is_truncated(value), active);
    }

    private static TtlField WithMapEntries(TtlField field, IReadOnlyList<TtlMapEntry> entries) =>
        new(field.Number, field.Name, field.JsonName, field.IsNamed, field.ValueType,
            field.WireType, field.ProtoType, field.Cardinality, field.IsMap,
            field.OneofName, field.OneofIndex, field.Value, field.ObjectValue,
            field.EnumType, field.EnumName, entries);

    private static TtlField CaptureField(nint field, int depth)
    {
        string? name = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_name(field));
        string? jsonName = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_json_name(field));
        var valueType = (TtlValueType)NativeMethods.ttl_field_type(field);
        var protoType = (ProtoType)NativeMethods.ttl_field_proto_type(field);
        bool isMap = NativeMethods.ttl_field_is_map(field);
        TtlObject? nested = null;
        object? value = null;
        string? enumType = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_enum_type(field));
        string? enumName = NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_enum_name(field));

        if (protoType == ProtoType.Message || valueType == TtlValueType.Message)
        {
            nint child = NativeMethods.ttl_field_get_object(field);
            if (child != 0) { nested = CaptureObject(child, depth + 1); value = nested; }
        }
        else if (protoType == ProtoType.Enum)
        {
            if (NativeMethods.ttl_field_get_enum_number(field, out int enumNumber)) value = enumNumber;
        }
        else
        {
            switch (protoType)
            {
                case ProtoType.Double: if (NativeMethods.ttl_field_get_f64(field, out double d)) value = d; break;
                case ProtoType.Float: if (NativeMethods.ttl_field_get_f32(field, out float f)) value = f; break;
                case ProtoType.Int64:
                case ProtoType.SInt64:
                case ProtoType.SFixed64: if (NativeMethods.ttl_field_get_i64(field, out long i64)) value = i64; break;
                case ProtoType.UInt64:
                case ProtoType.Fixed64: if (NativeMethods.ttl_field_get_u64(field, out ulong u64)) value = u64; break;
                case ProtoType.Int32:
                case ProtoType.SInt32:
                case ProtoType.SFixed32: if (NativeMethods.ttl_field_get_i32(field, out int i32)) value = i32; break;
                case ProtoType.UInt32:
                case ProtoType.Fixed32: if (NativeMethods.ttl_field_get_u32(field, out uint u32)) value = u32; break;
                case ProtoType.Bool: if (NativeMethods.ttl_field_get_bool(field, out bool boolean)) value = boolean; break;
                case ProtoType.String:
                    if (NativeMethods.ttl_field_get_string(field, out nint text, out nuint textLength))
                        value = NativeRuntime.ReadUtf8(text, textLength);
                    break;
                case ProtoType.Bytes:
                    if (NativeMethods.ttl_field_get_bytes(field, out nint bytes, out nuint byteLength))
                        value = new ReadOnlyMemory<byte>(NativeRuntime.CopyBytes(bytes, byteLength));
                    break;
                default:
                    value = valueType switch
                    {
                        TtlValueType.Varint when NativeMethods.ttl_field_get_u64(field, out ulong v) => v,
                        TtlValueType.Fixed32 when NativeMethods.ttl_field_get_u32(field, out uint v) => v,
                        TtlValueType.Fixed64 when NativeMethods.ttl_field_get_u64(field, out ulong v) => v,
                        TtlValueType.String when NativeMethods.ttl_field_get_string(field, out nint p, out nuint n) => NativeRuntime.ReadUtf8(p, n),
                        TtlValueType.Bytes when NativeMethods.ttl_field_get_bytes(field, out nint p, out nuint n) => new ReadOnlyMemory<byte>(NativeRuntime.CopyBytes(p, n)),
                        _ => null
                    };
                    break;
            }
        }

        return new TtlField(NativeMethods.ttl_field_number(field), name, jsonName,
            NativeMethods.ttl_field_is_named(field), valueType,
            NativeMethods.ttl_field_wire_type(field), protoType,
            (FieldCardinality)NativeMethods.ttl_field_cardinality(field), isMap,
            NativeRuntime.ReadNullTerminated(NativeMethods.ttl_field_oneof_name(field)),
            NativeMethods.ttl_field_oneof_index(field), value, nested, enumType,
            enumName, null);
    }

    private static NormalizedLiveEvent? CaptureNormalized(nint value, string method,
        int support, string? normalizedJson)
    {
        if (support != (int)EventSupport.Normalized || normalizedJson is null) return null;
        int kind = NativeMethods.ttl_event_normalized_type(value);
        if (kind == 1 && NativeMethods.ttl_event_chat(value, out NativeChatEvent chat))
        {
            NativeUser user = NativeMethods.ttl_event_get_user(value, out NativeUser commonUser) ? commonUser : chat.User;
            return new ChatLiveEvent(CopyUser(user), NativeRuntime.ReadUtf8(chat.Comment, chat.CommentLen));
        }
        if (kind == 2 && NativeMethods.ttl_event_gift(value, out NativeGiftEvent gift))
        {
            NativeUser user = NativeMethods.ttl_event_get_user(value, out NativeUser commonUser) ? commonUser : gift.User;
            return new GiftLiveEvent(CopyUser(user), gift.GiftId,
                NativeRuntime.ReadUtf8(gift.GiftName, gift.GiftNameLen), gift.DiamondCount,
                gift.RepeatCount, gift.ComboCount, gift.GroupId, gift.RepeatEnd != 0,
                NativeRuntime.ReadOptionalUtf8(gift.GiftImageUrl, gift.GiftImageUrlLen));
        }

        using JsonDocument document = JsonDocument.Parse(normalizedJson);
        JsonElement root = document.RootElement;
        return kind switch
        {
            3 => new LikeLiveEvent(ReadUser(root), ReadUInt64(root, "count"), ReadUInt64(root, "total")),
            4 => new MemberLiveEvent(ReadUser(root), ReadUInt64(root, "member_count"), ReadInt32(root, "action")),
            5 => new SocialLiveEvent(ReadUser(root), ReadInt64(root, "action"), ReadUInt64(root, "follow_count"), ReadUInt64(root, "share_count")),
            6 => ReadRoomUser(root),
            _ => null
        };
    }

    private static TtlUser CopyUser(NativeUser user) => new(user.Id,
        NativeRuntime.ReadUtf8(user.Nickname, user.NicknameLen),
        NativeRuntime.ReadUtf8(user.UniqueId, user.UniqueIdLen),
        NativeRuntime.ReadUtf8(user.SecUid, user.SecUidLen),
        NativeRuntime.ReadUtf8(user.AvatarUrl, user.AvatarUrlLen));

    private static TtlUser ReadUser(JsonElement element)
    {
        JsonElement user = element.GetProperty("user");
        return new TtlUser(ReadUInt64(user, "id"), ReadString(user, "nickname"),
            ReadString(user, "unique_id"), ReadString(user, "sec_uid"),
            user.TryGetProperty("avatar_url", out _) ? ReadString(user, "avatar_url") : string.Empty);
    }

    private static RoomUserLiveEvent ReadRoomUser(JsonElement root) => new(
        ReadUInt64(root, "total"), ReadUInt64(root, "popularity"), ReadUInt64(root, "total_user"),
        ReadUInt64(root, "anonymous"), ReadViewers(root, "top_viewers"), ReadViewers(root, "ranked_viewers"));

    private static IReadOnlyList<TopViewer> ReadViewers(JsonElement root, string property)
    {
        if (!root.TryGetProperty(property, out JsonElement items) || items.ValueKind != JsonValueKind.Array)
            return Array.Empty<TopViewer>();
        return Array.AsReadOnly(items.EnumerateArray().Select(item => new TopViewer(
            ReadUInt64(item, "rank"), ReadUInt64(item, "score"), ReadUInt64(item, "delta"), ReadUserFromViewer(item))).ToArray());
    }

    private static TtlUser ReadUserFromViewer(JsonElement item)
    {
        if (item.TryGetProperty("user", out JsonElement user))
            return new TtlUser(ReadUInt64(user, "id"), ReadOptionalString(user, "nickname"),
                ReadOptionalString(user, "unique_id"), ReadOptionalString(user, "sec_uid"), ReadOptionalString(user, "avatar_url"));
        return new TtlUser(ReadUInt64(item, "user_id"), ReadOptionalString(item, "nickname"),
            ReadOptionalString(item, "unique_id"), ReadOptionalString(item, "sec_uid"), ReadOptionalString(item, "avatar_url"));
    }

    private static string ReadString(JsonElement root, string name) => ReadOptionalString(root, name);
    private static string ReadOptionalString(JsonElement root, string name)
    {
        if (!root.TryGetProperty(name, out JsonElement value) || value.ValueKind == JsonValueKind.Null) return string.Empty;
        return value.ValueKind == JsonValueKind.String ? value.GetString() ?? string.Empty : value.ToString();
    }
    private static ulong ReadUInt64(JsonElement root, string name) =>
        ulong.TryParse(ReadOptionalString(root, name), out ulong result) ? result : 0;
    private static long ReadInt64(JsonElement root, string name) =>
        long.TryParse(ReadOptionalString(root, name), out long result) ? result : 0;
    private static int ReadInt32(JsonElement root, string name) =>
        int.TryParse(ReadOptionalString(root, name), out int result) ? result : 0;
}
