#!/usr/bin/env bash
#
# Re-vendor the TikTok Webcast v3 schemas at a chosen upstream commit.
#
#   scripts/update-tiktok-protos.sh <commit-ish>
#
# Schemas are deliberately NOT fetched during `cargo build`: a build that reaches
# the network is neither reproducible nor safe to run in CI, and a silent schema
# bump can change decoding behaviour in production. Updating is an explicit,
# reviewable step that ends in a commit.
#
# After this script succeeds, review `git diff` on the .proto tree before
# committing. Field-number changes deserve particular scrutiny — they are the
# only kind of change that alters the wire format.

set -euo pipefail

REPO="https://github.com/isaackogan/TikTok-Webcast-Protobuf"
UPSTREAM_PATH="src/slim/v3"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crate="$root/crates/ttl-live-proto"

if [[ $# -ne 1 ]]; then
    echo "usage: $(basename "$0") <commit-ish>" >&2
    echo >&2
    echo "current pin:" >&2
    sed -n 's/^commit=/  /p' "$crate/UPSTREAM" >&2
    exit 64
fi

ref="$1"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "==> cloning $REPO"
git clone --quiet --filter=blob:none "$REPO" "$work/upstream"
git -C "$work/upstream" checkout --quiet "$ref"

commit="$(git -C "$work/upstream" rev-parse HEAD)"
date="$(git -C "$work/upstream" show -s --format=%cs HEAD)"
echo "==> pinning $commit ($date)"

if [[ ! -d "$work/upstream/$UPSTREAM_PATH" ]]; then
    echo "error: $UPSTREAM_PATH does not exist at $commit" >&2
    exit 1
fi

# Replace wholesale so schemas deleted upstream disappear here too.
rm -rf "${crate:?}/proto/v3"
mkdir -p "$crate/proto/v3"
cp -R "$work/upstream/$UPSTREAM_PATH/." "$crate/proto/v3/"

# Upstream licence terms travel with the schemas; never strip them.
cp "$work/upstream/LICENSE" "$crate/LICENSE.upstream"

cat > "$crate/UPSTREAM" <<EOF
repository=$REPO
path=$UPSTREAM_PATH
commit=$commit
date=$date
license=AGPL-3.0-only
license_file=LICENSE.upstream

The contents of \`proto/v3/\` are copied verbatim from the upstream repository at
the commit pinned above. They are NOT covered by the MIT license that applies to
the rest of this workspace; see LICENSE.upstream and README.md in this crate.

To update, run \`scripts/update-tiktok-protos.sh <commit>\` from the repo root.
Never fetch schemas from \`main\` at build time.
EOF

# The commit is also compiled in, so a binary can report which schema it speaks.
sed -i "s/^pub const UPSTREAM_COMMIT: &str = \".*\";/pub const UPSTREAM_COMMIT: \&str = \"$commit\";/" \
    "$crate/src/lib.rs"

echo "==> building"
cargo build -p ttl-live-proto

echo "==> testing"
cargo test -p ttl-live-events

cat <<EOF

Pinned $commit.

Next:
  1. git diff -- crates/ttl-live-proto/proto   # review schema changes
  2. regenerate the Node goldens if normalisers changed:
       cd examples/node-connector && npx tsx golden-fixtures.ts
  3. cargo fmt && cargo test --workspace
  4. commit
EOF
