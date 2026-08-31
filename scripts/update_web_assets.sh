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
ALPINE_PAYLOAD_SRC="$RIM_ROOT/examples/wasm-synth/payload/alpine_payload.tar"
UEFI_PAYLOAD_SRC="$RIM_ROOT/examples/wasm-synth/payload/uefi_payload.tar"
WASM_DEST="$WEB_REPO/public/rim/wasm_synth.wasm"
ALPINE_PAYLOAD_DEST="$WEB_REPO/public/rim/payload/alpine_payload.tar"
UEFI_PAYLOAD_DEST="$WEB_REPO/public/rim/payload/uefi_payload.tar"

mkdir -p "$(dirname "$ALPINE_PAYLOAD_DEST")"

# 3. Copy
echo "[2/3] Copying artifacts to $WEB_REPO/public/rim/..."
cp -f "$WASM_SRC" "$WASM_DEST"
cp -f "$ALPINE_PAYLOAD_SRC" "$ALPINE_PAYLOAD_DEST"
if [[ -f "$UEFI_PAYLOAD_SRC" ]]; then
  cp -f "$UEFI_PAYLOAD_SRC" "$UEFI_PAYLOAD_DEST"
else
  echo "warning: UEFI payload TAR not found at $UEFI_PAYLOAD_SRC; keeping existing web payload if present." >&2
fi

echo "[3/3] Verifying copied artifacts:"
if [[ -f "$UEFI_PAYLOAD_DEST" ]]; then
  ls -lh "$WASM_DEST" "$ALPINE_PAYLOAD_DEST" "$UEFI_PAYLOAD_DEST"
else
  ls -lh "$WASM_DEST" "$ALPINE_PAYLOAD_DEST"
fi
echo "=== Web assets successfully updated! ==="
