#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RIM_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WEB_REPO="${1:-/mnt/d/Development/web/mki.dev}"

echo "=== Updating RIM Web Assets ==="
echo "RIM Root: $RIM_ROOT"
echo "Web Root: $WEB_REPO"

# 1. Build release WASM binary
echo ""
echo "[1/3] Building release WebAssembly module..."
cd "$RIM_ROOT"
cargo build -p wasm-synth --target wasm32-unknown-unknown --release

# 2. Paths
WASM_SRC="$RIM_ROOT/target/wasm32-unknown-unknown/release/wasm_synth.wasm"
PAYLOAD_SRC="$RIM_ROOT/examples/wasm-synth/payload/alpine_payload.tar"
WASM_DEST="$WEB_REPO/public/rim/wasm_synth.wasm"
PAYLOAD_DEST="$WEB_REPO/public/rim/payload/alpine_payload.tar"

mkdir -p "$(dirname "$PAYLOAD_DEST")"

# 3. Copy
echo "[2/3] Copying artifacts to $WEB_REPO/public/rim/..."
cp -f "$WASM_SRC" "$WASM_DEST"
cp -f "$PAYLOAD_SRC" "$PAYLOAD_DEST"

echo "[3/3] Verifying copied artifacts:"
ls -lh "$WASM_DEST" "$PAYLOAD_DEST"
echo "=== Web assets successfully updated! ==="
