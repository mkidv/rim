#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORK_DIR="$SCRIPT_DIR/../payload"

mkdir -p "$WORK_DIR/downloads"
mkdir -p "$WORK_DIR/esp/EFI/BOOT"
mkdir -p "$WORK_DIR/esp/loader/entries"
mkdir -p "$WORK_DIR/rootfs"

cd "$WORK_DIR/downloads"

echo "=== 1. Downloading Alpine 3.21 artifacts ==="
if [ ! -f "minirootfs.tar.gz" ]; then
    echo "Downloading Alpine minirootfs..."
    curl -fsSL -o minirootfs.tar.gz "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-minirootfs-3.21.3-x86_64.tar.gz"
fi

if [ ! -f "vmlinuz-virt" ]; then
    echo "Downloading vmlinuz-virt..."
    curl -fsSL -o vmlinuz-virt "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-virt"
fi

if [ ! -f "initramfs-virt" ]; then
    echo "Downloading initramfs-virt..."
    curl -fsSL -o initramfs-virt "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/initramfs-virt"
fi

if [ -f "/usr/lib/systemd/boot/efi/systemd-bootx64.efi" ]; then
    cp "/usr/lib/systemd/boot/efi/systemd-bootx64.efi" "$WORK_DIR/esp/EFI/BOOT/BOOTX64.EFI"
else
    echo "systemd-bootx64.efi not found"
    exit 1
fi

echo "=== 2. Setting up ESP partition content ==="
cp "vmlinuz-virt" "$WORK_DIR/esp/vmlinuz"
cp "initramfs-virt" "$WORK_DIR/esp/initramfs"

cat << 'EOCONF' > "$WORK_DIR/esp/loader/loader.conf"
default alpine.conf
timeout 1
console-mode max
EOCONF

cat << 'EOCONF' > "$WORK_DIR/esp/loader/entries/alpine.conf"
title Alpine Linux (RIM Storage Synthesized)
linux /vmlinuz
initrd /initramfs
options root=/dev/vda2 rw console=ttyS0 console=tty0 modules=virtio,ext4
EOCONF

echo "=== 3. Extracting and configuring rootfs ==="
rm -rf "$WORK_DIR/rootfs"/*
tar -xzf "minirootfs.tar.gz" -C "$WORK_DIR/rootfs"

sed -i 's/^root:[^:]*:/root::/' "$WORK_DIR/rootfs/etc/shadow"

cat << 'EOINIT' > "$WORK_DIR/rootfs/etc/inittab"
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

ttyS0::respawn:/bin/sh
tty1::respawn:/bin/sh

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOINIT

mkdir -p "$WORK_DIR/rootfs/etc/profile.d"
cat << 'EOPROFILE' > "$WORK_DIR/rootfs/etc/profile.d/rim-greeting.sh"
echo "======================================================"
echo " Welcome to Alpine Linux synthesized by RIM storage! "
echo " Kernel: $(uname -r) on $(uname -m)"
echo " Rootfs mounted on $(mount | grep ' / ' | awk '{print $1}')"
echo "======================================================"
EOPROFILE
chmod +x "$WORK_DIR/rootfs/etc/profile.d/rim-greeting.sh"

echo "=== 4. Packaging payload archive for WASM / host ==="
cd "$WORK_DIR"
tar -cf "alpine_payload.tar" esp rootfs
gzip -k -f "alpine_payload.tar"

echo "=== Staging Complete ==="
ls -lh "$WORK_DIR/alpine_payload.tar.gz"