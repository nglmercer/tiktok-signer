#!/usr/bin/env python3
"""Validate every public C declaration against its .NET P/Invoke signature."""
from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
HEADERS = ROOT / "crates/ttl-live-native/include/ttl"
METHODS = ROOT / "sdk/dotnet/src/Ttl.Live/Native/NativeMethods.cs"
CALLBACKS = ROOT / "sdk/dotnet/src/Ttl.Live/Native/NativeCallbacks.cs"

ENUM_TYPES = {
    "ttl_result_t", "ttl_connection_state_t", "ttl_event_support_t",
    "ttl_value_type_t", "ttl_proto_type_t", "ttl_field_cardinality_t",
    "ttl_normalized_type_t", "ttl_json_mode_t",
}
PRIMITIVE_TYPES = {
    "void": "void", "bool": "bool", "char": "nint", "uint8_t": "byte",
    "uint32_t": "uint", "uint64_t": "ulong", "int32_t": "int",
    "int64_t": "long", "size_t": "nuint", "float": "float", "double": "double",
    "ttl_event_type_t": "uint",
}
OPAQUE_TYPES = {
    "ttl_live_client_t", "ttl_event_t", "ttl_batch_t", "ttl_object_t",
    "ttl_field_t", "ttl_event_schema_t", "ttl_schema_field_t",
}
STRUCT_TYPES = {
    "ttl_live_options_t": "NativeLiveOptions",
    "ttl_error_info_t": "NativeErrorInfo",
    "ttl_event_info_t": "NativeEventInfo",
    "ttl_user_t": "NativeUser",
    "ttl_chat_event_t": "NativeChatEvent",
    "ttl_gift_event_t": "NativeGiftEvent",
}
CALLBACK_TYPES = {
    "ttl_event_callback_t", "ttl_batch_callback_t",
    "ttl_state_callback_t", "ttl_error_callback_t",
}
CALLBACK_DELEGATES = {
    "ttl_event_callback_t": "EventDelegate",
    "ttl_batch_callback_t": "BatchDelegate",
    "ttl_state_callback_t": "StateDelegate",
    "ttl_error_callback_t": "ErrorDelegate",
}


def strip_comments(source: str) -> str:
    source = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
    return re.sub(r"^[ \t]*#.*$", "", source, flags=re.M)


def public_declarations() -> dict[str, tuple[str, list[tuple[str, str]]]]:
    declarations: dict[str, tuple[str, list[tuple[str, str]]]] = {}
    pattern = re.compile(r"TTL_API\s+(.+?)(ttl_\w+)\s*\((.*?)\)\s*;", re.S)
    for path in HEADERS.glob("*.h"):
        source = strip_comments(path.read_text(encoding="utf-8"))
        for match in pattern.finditer(source):
            result, name, raw_params = match.groups()
            result = " ".join(result.split())
            params: list[tuple[str, str]] = []
            if raw_params.strip() not in ("", "void"):
                for raw in raw_params.split(","):
                    raw = " ".join(raw.split())
                    parsed = re.fullmatch(r"(.+?)([A-Za-z_]\w*)", raw)
                    if not parsed:
                        raise ValueError(f"cannot parse C parameter {raw!r} in {name}")
                    c_type, param_name = parsed.groups()
                    params.append((" ".join(c_type.split()), param_name))
            if name in declarations:
                raise ValueError(f"duplicate public C declaration: {name}")
            declarations[name] = (result, params)
    return declarations


def c_base_type(c_type: str) -> tuple[str, int]:
    stars = c_type.count("*")
    base = " ".join(c_type.replace("*", " ").replace("const", " ").split())
    return base, stars


def managed_type_for_c(base: str) -> tuple[str, bool]:
    if base in PRIMITIVE_TYPES:
        return PRIMITIVE_TYPES[base], base == "bool"
    if base in ENUM_TYPES:
        return "int", False
    if base in OPAQUE_TYPES or base in STRUCT_TYPES or base in CALLBACK_TYPES:
        return "nint", False
    raise ValueError(f"unknown C ABI type {base!r}")


def expected_parameter(function: str, index: int, c_type: str, name: str) -> tuple[str, str, bool]:
    base, stars = c_base_type(c_type)
    if stars == 0:
        managed, bool_marshaling = managed_type_for_c(base)
        return "", managed, bool_marshaling
    if base in STRUCT_TYPES and stars == 1:
        if base == "ttl_live_options_t":
            return "in", STRUCT_TYPES[base], False
        return "out", STRUCT_TYPES[base], False
    if stars == 1 and c_type.strip().startswith("const ") and base in ("uint8_t", "char"):
        return "", "nint", False
    if stars == 1 and base in PRIMITIVE_TYPES and base not in ("void", "char"):
        managed, bool_marshaling = managed_type_for_c(base)
        return "out", managed, bool_marshaling
    if stars >= 2 and base in ("char", "uint8_t") and name in ("out", "result"):
        return "out", "nint", False
    # ttl_object_find_all writes into caller-provided pointer storage, not an out parameter.
    if function == "ttl_object_find_all" and index == 3:
        return "", "nint", False
    return "", "nint", False


def parse_managed_imports() -> dict[str, dict[str, object]]:
    source = METHODS.read_text(encoding="utf-8")
    pattern = re.compile(
        r"\[DllImport\((?P<attrs>[^\]]+)\)\](?P<return_attrs>(?:\s*\[return:[^\]]+\])*)"
        r"\s*internal\s+static\s+extern\s+(?P<result>[A-Za-z_]\w*)\s+"
        r"(?P<name>ttl_\w+)\((?P<params>[^;]*)\);",
        re.S,
    )
    imports: dict[str, dict[str, object]] = {}
    for match in pattern.finditer(source):
        attrs = match.group("attrs")
        result_attrs = match.group("return_attrs")
        params: list[tuple[str, str, bool]] = []
        raw_params = match.group("params").strip()
        if raw_params:
            for raw in raw_params.split(","):
                marshaled = "MarshalAs(UnmanagedType.I1)" in raw
                raw = re.sub(r"\[MarshalAs\([^]]+\)\]\s*", "", raw).strip()
                parsed = re.fullmatch(r"(?:(in|out|ref)\s+)?([A-Za-z_]\w*)\s+[A-Za-z_]\w*", raw)
                if not parsed:
                    raise ValueError(f"cannot parse C# import parameter {raw!r} in {match.group('name')}")
                direction, managed_type = parsed.groups()
                params.append((direction or "", managed_type, marshaled))
        entrypoint_match = re.search(r'EntryPoint\s*=\s*"(ttl_\w+)"', attrs)
        imports[match.group("name")] = {
            "entrypoint": entrypoint_match.group(1) if entrypoint_match else None,
            "attrs": attrs,
            "result": match.group("result"),
            "result_i1": "MarshalAs(UnmanagedType.I1)" in result_attrs,
            "params": params,
        }
    return imports


def parse_callbacks() -> tuple[dict[str, tuple[str, list[str], str]], dict[str, tuple[str, list[str]]]]:
    header = strip_comments("\n".join(path.read_text(encoding="utf-8") for path in HEADERS.glob("*.h")))
    declarations: dict[str, tuple[str, list[str]]] = {}
    pattern = re.compile(r"typedef\s+([A-Za-z_]\w*)\s*\(\s*\*\s*(ttl_\w+_callback_t)\s*\)\s*\((.*?)\)\s*;", re.S)
    for result, name, raw_params in pattern.findall(header):
        params: list[str] = []
        for raw in raw_params.split(","):
            parsed = re.fullmatch(r"(.+?)([A-Za-z_]\w*)", " ".join(raw.split()))
            if not parsed:
                raise ValueError(f"cannot parse callback parameter {raw!r} for {name}")
            params.append(" ".join(parsed.group(1).split()))
        declarations[name] = (" ".join(result.split()), params)

    managed_source = CALLBACKS.read_text(encoding="utf-8")
    managed: dict[str, tuple[str, list[str], str]] = {}
    delegate_pattern = re.compile(
        r"\[UnmanagedFunctionPointer\(CallingConvention\.(\w+)\)\]\s*"
        r"private\s+delegate\s+([A-Za-z_]\w*)\s+([A-Za-z_]\w*)\(([^)]*)\);",
        re.S,
    )
    for convention, result, name, raw_params in delegate_pattern.findall(managed_source):
        params = []
        if raw_params.strip():
            for raw in raw_params.split(","):
                parsed = re.fullmatch(r"(?:(in|out|ref)\s+)?([A-Za-z_]\w*)\s+[A-Za-z_]\w*", raw.strip())
                if not parsed:
                    raise ValueError(f"cannot parse managed callback parameter {raw!r} in {name}")
                direction, managed_type = parsed.groups()
                params.append(f"{direction or ''}:{managed_type}")
        managed[name] = (result, params, convention)
    return managed, declarations


def callback_type(c_type: str) -> tuple[str, bool]:
    base, stars = c_base_type(c_type)
    managed, bool_marshaling = managed_type_for_c(base)
    if stars:
        return "nint", False
    return managed, bool_marshaling


def check_signatures() -> list[str]:
    errors: list[str] = []
    declarations = public_declarations()
    imports = parse_managed_imports()
    for name in sorted(declarations.keys() - imports.keys()):
        errors.append(f"missing P/Invoke: {name}")
    for name in sorted(imports.keys() - declarations.keys()):
        errors.append(f"P/Invoke absent from headers: {name}")

    for name in sorted(declarations.keys() & imports.keys()):
        c_result, c_params = declarations[name]
        managed = imports[name]
        if managed["entrypoint"] != name:
            errors.append(f"{name}: EntryPoint is {managed['entrypoint']!r}, expected exact symbol name")
        attrs = str(managed["attrs"])
        if "CallingConvention.Cdecl" not in attrs:
            errors.append(f"{name}: missing explicit Cdecl calling convention")
        if "ExactSpelling = true" not in attrs:
            errors.append(f"{name}: missing ExactSpelling = true")

        c_base, c_stars = c_base_type(c_result)
        expected_result, return_bool = managed_type_for_c(c_base)
        if c_stars:
            expected_result, return_bool = "nint", False
        if managed["result"] != expected_result:
            errors.append(f"{name}: return type is {managed['result']}, expected {expected_result} for {c_result}")
        if managed["result_i1"] != return_bool:
            errors.append(f"{name}: return C bool must use [return: MarshalAs(UnmanagedType.I1)]" if return_bool
                          else f"{name}: unexpected bool return marshaling")

        managed_params = managed["params"]
        if len(managed_params) != len(c_params):
            errors.append(f"{name}: parameter count is {len(managed_params)}, expected {len(c_params)}")
            continue
        for index, ((c_type, c_name), (direction, actual_type, actual_i1)) in enumerate(zip(c_params, managed_params)):
            expected = expected_parameter(name, index, c_type, c_name)
            if (direction, actual_type, actual_i1) != expected:
                errors.append(
                    f"{name} parameter {index + 1} ({c_name}): managed {(direction, actual_type, actual_i1)} "
                    f"does not match C {c_type} {c_name}, expected {expected}"
                )

    managed_callbacks, c_callbacks = parse_callbacks()
    if set(c_callbacks) != set(CALLBACK_DELEGATES):
        errors.append(f"header callback inventory mismatch: {sorted(c_callbacks)}")
    for c_name, delegate_name in CALLBACK_DELEGATES.items():
        if c_name not in c_callbacks or delegate_name not in managed_callbacks:
            errors.append(f"missing callback binding: {c_name} -> {delegate_name}")
            continue
        c_result, c_params = c_callbacks[c_name]
        managed_result, managed_params, convention = managed_callbacks[delegate_name]
        expected_result, _ = callback_type(c_result)
        expected_params = [f":{callback_type(param)[0]}" for param in c_params]
        if convention != "Cdecl": errors.append(f"{delegate_name}: callback calling convention is {convention}, expected Cdecl")
        if managed_result != expected_result: errors.append(f"{delegate_name}: callback return mismatch")
        if managed_params != expected_params: errors.append(f"{delegate_name}: callback parameters {managed_params} != {expected_params}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--library", type=pathlib.Path)
    args = parser.parse_args()
    errors = check_signatures()
    expected_count = len(public_declarations())
    actual_count = len(parse_managed_imports())
    if errors:
        print(f"header declarations: {expected_count}; managed imports: {actual_count}")
        print("\n".join(errors))
        return 1
    if expected_count != 98 or actual_count != 98:
        print(f"Expected the preserved 98-function interop surface; found {expected_count} C declarations and {actual_count} imports")
        return 1

    if args.library:
        check = ROOT / "scripts/native/check-exports.py"
        result = subprocess.run([sys.executable, str(check), str(args.library)], check=False)
        if result.returncode:
            return result.returncode
    print("All 98 native declarations match managed return/parameter types, order, directions, Cdecl, and C bool marshaling.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
