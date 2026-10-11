#!/usr/bin/env bash
set -euo pipefail
relay_repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../.." && pwd)"
if ! command -v dotnet >/dev/null; then
    if [[ -x /tmp/codex-dotnet/dotnet ]]; then export PATH="/tmp/codex-dotnet:$PATH"; fi
fi
if ! command -v dotnet >/dev/null; then
    echo 'Install .NET SDK 8 or 10 and put dotnet on PATH.' >&2
    exit 1
fi
export TTL_LIVE_NATIVE_LIBRARY="${TTL_LIVE_NATIVE_LIBRARY:-$relay_repo_root/target/release/libttl_live.so}"
export TTL_BUNDLE="${TTL_BUNDLE:-${XDG_CONFIG_HOME:-$HOME/.config}/ttl-signer/webmssdk.js}"
export LIVE_FIXTURES_DIRECTORY="$relay_repo_root/fixtures/events"
export ASPNETCORE_URLS="${ASPNETCORE_URLS:-http://127.0.0.1:15050}"
relay_session_file="${XDG_CONFIG_HOME:-$HOME/.config}/ttl-signer/session"
if [[ -z "${TTL_SESSION_FILE:-}" && -f "$relay_session_file" ]]; then export TTL_SESSION_FILE="$relay_session_file"; fi
cd "$relay_repo_root"
exec dotnet run --project sdk/dotnet/examples/AspNetCoreRelay -c Release --no-launch-profile
