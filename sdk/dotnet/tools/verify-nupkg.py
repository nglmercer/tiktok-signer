#!/usr/bin/env python3
"""Fail if the Ttl.Live nupkg omits a framework or native runtime asset."""
from __future__ import annotations

import argparse
import pathlib
import sys
import zipfile

EXPECTED = {
    "runtimes/win-x64/native/ttl_live.dll",
    "runtimes/linux-x64/native/libttl_live.so",
    "runtimes/linux-arm64/native/libttl_live.so",
    "runtimes/osx-x64/native/libttl_live.dylib",
    "runtimes/osx-arm64/native/libttl_live.dylib",
    "lib/net8.0/Ttl.Live.dll",
    "lib/net10.0/Ttl.Live.dll",
    "README.md",
    "THIRD-PARTY-NOTICES/LICENSE.upstream",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("package", type=pathlib.Path)
    args = parser.parse_args()
    with zipfile.ZipFile(args.package) as package:
        names = set(package.namelist())
    missing = sorted(EXPECTED - names)
    if missing:
        print("nupkg is missing required assets: " + ", ".join(missing), file=sys.stderr)
        return 1
    print(f"NuGet package contains all {len(EXPECTED)} required managed, native, and notice assets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
