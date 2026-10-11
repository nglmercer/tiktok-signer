#!/usr/bin/env python3
"""Check that every public native header declaration has a Cdecl .NET import."""
from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
HEADERS = ROOT / "crates/ttl-live-native/include/ttl"
METHODS = ROOT / "sdk/dotnet/src/Ttl.Live/Native/NativeMethods.cs"


def declared_symbols() -> set[str]:
    symbols: set[str] = set()
    pattern = re.compile(r"TTL_API\s+[^;]+?\b(ttl_\w+)\s*\(")
    for path in HEADERS.glob("*.h"):
        symbols.update(pattern.findall(path.read_text(encoding="utf-8")))
    return symbols


def managed_symbols() -> tuple[set[str], int]:
    source = METHODS.read_text(encoding="utf-8")
    imports = re.findall(
        r"\[DllImport\([^\]]*?EntryPoint\s*=\s*\"(ttl_\w+)\"[^\]]*?\)\]"
        r"\s*(?:\[return:\s*MarshalAs\([^\]]+\)\]\s*)?"
        r"internal\s+static\s+extern\s+[^;]+;",
        source,
    )
    declarations = set(imports)
    cdecl_count = source.count("CallingConvention = CallingConvention.Cdecl")
    return declarations, cdecl_count


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--library", type=pathlib.Path)
    args = parser.parse_args()
    expected = declared_symbols()
    actual, cdecl_count = managed_symbols()
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    if missing or extra or cdecl_count != len(actual):
        print(f"header declarations: {len(expected)}; managed imports: {len(actual)}; Cdecl imports: {cdecl_count}")
        if missing:
            print("missing managed imports:", ", ".join(missing))
        if extra:
            print("imports absent from public headers:", ", ".join(extra))
        if cdecl_count != len(actual):
            print("one or more imports do not explicitly use CallingConvention.Cdecl")
        return 1

    if args.library:
        check = ROOT / "scripts/native/check-exports.py"
        result = subprocess.run([sys.executable, str(check), str(args.library)], check=False)
        if result.returncode:
            return result.returncode
    print(f"{len(expected)} native declarations have matching explicit Cdecl .NET imports")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
