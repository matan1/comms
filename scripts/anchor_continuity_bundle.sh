#!/usr/bin/env sh
# Pack the continuity trial store into one sealed, portable bundle and verify it
# with the Python-free `comms-verify` binary. This is the Article-5 anchoring
# artifact: a single file anyone can check offline.
#
# Usage:
#   scripts/anchor_continuity_bundle.sh [steward-key.json] [out-bundle]
#
# With no key, a throwaway demo steward is minted (the bundle is still fully
# verifiable; for a durable anchor, pass the historian's steward key file).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$ROOT/rust/target/release/comms-verify"
KEY="${1:-}"
OUT="${2:-$ROOT/continuity.bundle}"

if [ ! -x "$BIN" ]; then
    echo "building comms-verify..."
    ( cd "$ROOT/rust" && cargo build --release )
fi

if [ -z "$KEY" ]; then
    KEY="$ROOT/.continuity-demo.steward"
    echo "no key given; minting a throwaway demo steward at $KEY"
    "$BIN" mint --out "$KEY" --label "continuity anchor (demo)"
fi

DESC="continuity trial store — $(ls "$ROOT/continuity/store"/*.cbor | wc -l | tr -d ' ') attestations"
"$BIN" pack --out "$OUT" "$ROOT/continuity/store" --seal --key "$KEY" --description "$DESC"

echo
echo "=== verify (seal) ==="
"$BIN" verify "$OUT"
echo
echo "=== inspect (every member) ==="
"$BIN" inspect "$OUT"
