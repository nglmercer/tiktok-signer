#!/usr/bin/env python3
"""Fail if the Ttl.Live nupkg omits a framework or native runtime asset."""
from __future__ import annotations

import argparse
import pathlib
import sys
import zipfile

RUNTIME_ASSETS = {
    "win-x64": "runtimes/win-x64/native/ttl_live.dll",
    "linux-x64": "runtimes/linux-x64/native/libttl_live.so",
    "linux-arm64": "runtimes/linux-arm64/native/libttl_live.so",
    "osx-x64": "runtimes/osx-x64/native/libttl_live.dylib",
    "osx-arm64": "runtimes/osx-arm64/native/libttl_live.dylib",
}
BASE_EXPECTED = {
    "lib/net8.0/Ttl.Live.dll",
    "lib/net10.0/Ttl.Live.dll",
    "README.md",
    "PACKAGE-LICENSE.txt",
    "THIRD-PARTY-NOTICES/LICENSE.upstream",
    "THIRD-PARTY-NOTICES/LICENSE",
    "THIRD-PARTY-NOTICES/README.protobuf.md",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("package", type=pathlib.Path)
    parser.add_argument("--rid", choices=sorted(RUNTIME_ASSETS),
                        help="check one RID for a local partial package; CI checks all five when omitted")
    args = parser.parse_args()
    with zipfile.ZipFile(args.package) as package:
        names = set(package.namelist())
        license_notice = " ".join(package.read("PACKAGE-LICENSE.txt").decode("utf-8").split()) if "PACKAGE-LICENSE.txt" in names else ""
        upstream_notice = " ".join(package.read("THIRD-PARTY-NOTICES/LICENSE.upstream").decode("utf-8").split()) if "THIRD-PARTY-NOTICES/LICENSE.upstream" in names else ""
    expected = BASE_EXPECTED | (set(RUNTIME_ASSETS.values()) if args.rid is None else {RUNTIME_ASSETS[args.rid]})
    missing = sorted(expected - names)
    if missing:
        print("nupkg is missing required assets: " + ", ".join(missing), file=sys.stderr)
        return 1
    for marker in ("not be treated as MIT-only", "AGPL-3.0", "Section 19"):
        if marker not in license_notice:
            print(f"PACKAGE-LICENSE.txt is missing required disclosure: {marker}", file=sys.stderr)
            return 1
    if "Additional Permissions & Exceptions" not in upstream_notice:
        print("nupkg upstream protobuf license is missing its additional permissions", file=sys.stderr)
        return 1
    scope = "all five RIDs" if args.rid is None else args.rid
    print(f"NuGet package contains all required managed, notice, and {scope} assets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
