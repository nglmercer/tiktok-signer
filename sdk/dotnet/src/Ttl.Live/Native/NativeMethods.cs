using System.Reflection;
using System.Runtime.InteropServices;

namespace Ttl.Live.Native;

/// <summary>Exact C ABI imports for the public ttl-live-native headers.</summary>
internal static partial class NativeMethods
{
    internal const string Library = "ttl_live";

    static NativeMethods() => NativeLibraryResolver.Register(typeof(NativeMethods).Assembly);

    [DllImport(Library, EntryPoint = "ttl_live_abi_version", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_live_abi_version();
    [DllImport(Library, EntryPoint = "ttl_live_version", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_live_version();
    [DllImport(Library, EntryPoint = "ttl_last_error", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_last_error();
    [DllImport(Library, EntryPoint = "ttl_last_error_code", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_last_error_code();
    [DllImport(Library, EntryPoint = "ttl_string_free", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern void ttl_string_free(nint value);

    [DllImport(Library, EntryPoint = "ttl_live_client_create", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_live_client_create(in NativeLiveOptions options);
    [DllImport(Library, EntryPoint = "ttl_live_client_connect", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_connect(nint client);
    [DllImport(Library, EntryPoint = "ttl_live_client_disconnect", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_disconnect(nint client);
    [DllImport(Library, EntryPoint = "ttl_live_client_destroy", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern void ttl_live_client_destroy(nint client);
    [DllImport(Library, EntryPoint = "ttl_live_client_set_event_callback", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_set_event_callback(nint client, nint callback, nint userdata);
    [DllImport(Library, EntryPoint = "ttl_live_client_set_batch_callback", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_set_batch_callback(nint client, nint callback, nint userdata);
    [DllImport(Library, EntryPoint = "ttl_live_client_set_state_callback", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_set_state_callback(nint client, nint callback, nint userdata);
    [DllImport(Library, EntryPoint = "ttl_live_client_set_error_callback", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_live_client_set_error_callback(nint client, nint callback, nint userdata);

    [DllImport(Library, EntryPoint = "ttl_event_decode", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_decode(nint method, nuint methodLen, ulong messageId, [MarshalAs(UnmanagedType.I1)] bool isHistory, nint payload, nuint payloadLen);
    [DllImport(Library, EntryPoint = "ttl_event_clone", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_clone(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_free", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern void ttl_event_free(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_method", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_method(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_method_len", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_event_method_len(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_event_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_message_id", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern ulong ttl_event_message_id(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_is_history", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_is_history(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_support", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_event_support(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_normalized_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_event_normalized_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_schema_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_schema_name(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_raw_size", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_event_raw_size(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_raw_data", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_raw_data(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_root", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_root(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_is_truncated", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_is_truncated(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_flags", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_event_flags(nint value);
    [DllImport(Library, EntryPoint = "ttl_event_timestamp", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_timestamp(nint value, out ulong timestamp);
    [DllImport(Library, EntryPoint = "ttl_event_get_info", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_get_info(nint value, out NativeEventInfo info);
    [DllImport(Library, EntryPoint = "ttl_event_to_json", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_to_json(nint value, int mode);

    [DllImport(Library, EntryPoint = "ttl_batch_decode", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_batch_decode(nint payload, nuint payloadLen);
    [DllImport(Library, EntryPoint = "ttl_batch_free", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern void ttl_batch_free(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_event_count", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_batch_event_count(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_event_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_batch_event_at(nint value, nuint index);
    [DllImport(Library, EntryPoint = "ttl_batch_cursor", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_batch_cursor(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_internal_ext", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_batch_internal_ext(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_push_server", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_batch_push_server(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_need_ack", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_batch_need_ack(nint value);
    [DllImport(Library, EntryPoint = "ttl_batch_heartbeat_duration", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern long ttl_batch_heartbeat_duration(nint value);

    [DllImport(Library, EntryPoint = "ttl_object_field_count", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_object_field_count(nint value);
    [DllImport(Library, EntryPoint = "ttl_object_field_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_object_field_at(nint value, nuint index);
    [DllImport(Library, EntryPoint = "ttl_object_find", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_object_find(nint value, nint name, nuint nameLen);
    [DllImport(Library, EntryPoint = "ttl_object_find_all", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_object_find_all(nint value, nint name, nuint nameLen, nint output, nuint capacity);
    [DllImport(Library, EntryPoint = "ttl_object_is_truncated", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_object_is_truncated(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_number", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_field_number(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_name(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_json_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_json_name(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_field_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_wire_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern byte ttl_field_wire_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_proto_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_field_proto_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_is_named", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_is_named(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_cardinality", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_field_cardinality(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_is_map", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_is_map(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_oneof_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_oneof_name(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_oneof_index", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_field_oneof_index(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_get_u64", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_u64(nint value, out ulong result);
    [DllImport(Library, EntryPoint = "ttl_field_get_u32", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_u32(nint value, out uint result);
    [DllImport(Library, EntryPoint = "ttl_field_get_i64", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_i64(nint value, out long result);
    [DllImport(Library, EntryPoint = "ttl_field_get_i32", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_i32(nint value, out int result);
    [DllImport(Library, EntryPoint = "ttl_field_get_f64", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_f64(nint value, out double result);
    [DllImport(Library, EntryPoint = "ttl_field_get_f32", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_f32(nint value, out float result);
    [DllImport(Library, EntryPoint = "ttl_field_get_bool", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_bool(nint value, [MarshalAs(UnmanagedType.I1)] out bool result);
    [DllImport(Library, EntryPoint = "ttl_field_get_string", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_string(nint value, out nint result, out nuint length);
    [DllImport(Library, EntryPoint = "ttl_field_get_bytes", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_bytes(nint value, out nint result, out nuint length);
    [DllImport(Library, EntryPoint = "ttl_field_get_object", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_get_object(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_get_enum_number", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_field_get_enum_number(nint value, out int result);
    [DllImport(Library, EntryPoint = "ttl_field_enum_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_enum_type(nint value);
    [DllImport(Library, EntryPoint = "ttl_field_enum_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_field_enum_name(nint value);

    [DllImport(Library, EntryPoint = "ttl_event_get_user", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_get_user(nint value, out NativeUser user);
    [DllImport(Library, EntryPoint = "ttl_event_chat", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_chat(nint value, out NativeChatEvent chat);
    [DllImport(Library, EntryPoint = "ttl_event_gift", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_event_gift(nint value, out NativeGiftEvent gift);
    [DllImport(Library, EntryPoint = "ttl_object_map_size", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_object_map_size(nint value, nint name, nuint nameLen);
    [DllImport(Library, EntryPoint = "ttl_object_map_key_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_object_map_key_at(nint value, nint name, nuint nameLen, nuint index);
    [DllImport(Library, EntryPoint = "ttl_object_map_value_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_object_map_value_at(nint value, nint name, nuint nameLen, nuint index);
    [DllImport(Library, EntryPoint = "ttl_object_oneof_active", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_object_oneof_active(nint value, nint name, nuint nameLen);

    [DllImport(Library, EntryPoint = "ttl_schema_event_count", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_schema_event_count();
    [DllImport(Library, EntryPoint = "ttl_schema_event_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_event_at(nuint index);
    [DllImport(Library, EntryPoint = "ttl_schema_event_find", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_event_find(nint method, nuint methodLen);
    [DllImport(Library, EntryPoint = "ttl_schema_fingerprint", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_fingerprint();
    [DllImport(Library, EntryPoint = "ttl_event_schema_method", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_schema_method(nint schema);
    [DllImport(Library, EntryPoint = "ttl_event_schema_full_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_schema_full_name(nint schema);
    [DllImport(Library, EntryPoint = "ttl_event_schema_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_event_schema_type(nint schema);
    [DllImport(Library, EntryPoint = "ttl_event_schema_field_count", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nuint ttl_event_schema_field_count(nint schema);
    [DllImport(Library, EntryPoint = "ttl_event_schema_field_at", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_event_schema_field_at(nint schema, nuint index);
    [DllImport(Library, EntryPoint = "ttl_schema_field_number", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern uint ttl_schema_field_number(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_field_name(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_json_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_field_json_name(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_proto_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_schema_field_proto_type(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_wire_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern byte ttl_schema_field_wire_type(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_cardinality", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_schema_field_cardinality(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_is_map", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_schema_field_is_map(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_is_deprecated", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)][return: MarshalAs(UnmanagedType.I1)] internal static extern bool ttl_schema_field_is_deprecated(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_oneof_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_field_oneof_name(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_oneof_index", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern int ttl_schema_field_oneof_index(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_message_name", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_field_message_name(nint field);
    [DllImport(Library, EntryPoint = "ttl_schema_field_enum_type", ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)] internal static extern nint ttl_schema_field_enum_type(nint field);
}
