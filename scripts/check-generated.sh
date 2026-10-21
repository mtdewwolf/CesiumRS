#!/usr/bin/env bash
# Freshness check for the checked-in generated files under
# web/client/generated. Rebuilds the wasm and re-embeds it (and the Earth
# texture) into a temporary directory, then compares byte for byte with the
# committed copies. Exits non-zero when they differ, so a Rust or asset
# change that was not followed by scripts/build-web.sh cannot slip through.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

bash scripts/build-web.sh "$tmp" >/dev/null

stale=0
for f in wasm.ts earth.ts; do
  if ! cmp -s "$tmp/$f" "web/client/generated/$f"; then
    echo "stale: web/client/generated/$f does not match a fresh build" >&2
    stale=1
  fi
done

if [ "$stale" -ne 0 ]; then
  echo "Run scripts/build-web.sh and commit the regenerated files." >&2
  exit 1
fi
echo "web/client/generated is up to date"
