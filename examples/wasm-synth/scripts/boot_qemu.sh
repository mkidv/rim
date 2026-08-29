#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$SCRIPT_DIR/../../.."
DISK_IMG="$WORKSPACE_ROOT/target/rim-alpine-boot.img"

if [ $# -ge 1 ]; then
    DISK_IMG="$1"
fi

if [ ! -f "$DISK_IMG" ]; then
    echo "Error: Disk image not found at $DISK_IMG"
    exit 1
fi

OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
if [ ! -f "$OVMF_BIOS" ]; then
    if [ -f "/usr/share/OVMF/OVMF_CODE.fd" ]; then
        OVMF_BIOS="/usr/share/OVMF/OVMF_CODE.fd"
    else
        echo "Error: OVMF UEFI firmware not found. Install ovmf package."
        exit 1
    fi
fi

ACCEL_OPTS=""
if [ -e "/dev/kvm" ] && [ -r "/dev/kvm" ] && [ -w "/dev/kvm" ]; then
    ACCEL_OPTS="-enable-kvm -cpu host"
    echo "Using KVM hardware acceleration"
else
    ACCEL_OPTS="-cpu qemu64"
    echo "Using QEMU TCG emulation"
fi

echo "=================================================="
echo " Booting RIM-Synthesized Alpine Linux under UEFI  "
echo " Image: $DISK_IMG"
echo " OVMF:  $OVMF_BIOS"
echo " Press Ctrl+A then X to exit QEMU"
echo "=================================================="

qemu-system-x86_64 \
    $ACCEL_OPTS \
    -m 512M \
    -bios "$OVMF_BIOS" \
    -drive file="$DISK_IMG",format=raw,if=virtio \
    -nographic \
    -serial mon:stdio