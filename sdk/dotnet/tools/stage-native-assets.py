#!/usr/bin/env python3
"""Stage native CI artifacts into NuGet's RID-aware runtime asset layout."""
from __future__ import annotations

import argparse
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
ARTIFACTS = {
    "x86_64-pc-windows-msvc": ("win-x64", "ttl_live.dll"),
    "x86_64-unknown-linux-gnu": ("linux-x64", "libttl_live.so"),
    "aarch64-unknown-linux-gnu": ("linux-arm64", "libttl_live.so"),
    "x86_64-apple-darwin": ("osx-x64", "libttl_live.dylib"),
    "aarch64-apple-darwin": ("osx-arm64", "libttl_live.dylib"),
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact_root", type=pathlib.Path)
    args = parser.parse_args()
    runtime_root = ROOT / "sdk/dotnet/src/Ttl.Live/runtimes"
    if runtime_root.exists():
        shutil.rmtree(runtime_root)

    missing = []
    for target, (rid, library) in ARTIFACTS.items():
        source = args.artifact_root / f"ttl-live-{target}" / library
        if not source.is_file():
            missing.append(str(source))
            continue
        destination = runtime_root / rid / "native" / library
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    if missing:
        print("missing native artifacts:\n" + "\n".join(missing), file=sys.stderr)
        return 1
    print(f"Staged {len(ARTIFACTS)} RID native libraries under {runtime_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
