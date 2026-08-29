#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORK_DIR="$SCRIPT_DIR/../payload"
ROOTFS="/tmp/alpine_rootfs"

rm -rf "$WORK_DIR/esp" "$WORK_DIR/rootfs"
mkdir -p "$WORK_DIR/esp/EFI/BOOT" "$WORK_DIR/esp/loader/entries" "$WORK_DIR/rootfs"

echo "=== 1. Setting up ESP ==="
cp "/usr/lib/systemd/boot/efi/systemd-bootx64.efi" "$WORK_DIR/esp/EFI/BOOT/BOOTX64.EFI"
cp "$ROOTFS/boot/vmlinuz-virt" "$WORK_DIR/esp/vmlinuz"
cp "$ROOTFS/boot/initramfs-virt" "$WORK_DIR/esp/initramfs"

cat << 'EOCONF' > "$WORK_DIR/esp/loader/loader.conf"
default alpine.conf
timeout 1
console-mode max
EOCONF

cat << 'EOCONF' > "$WORK_DIR/esp/loader/entries/alpine.conf"
title Alpine Linux (RIM Storage Synthesized)
linux /vmlinuz
initrd /initramfs
options root=/dev/vda2 rw console=ttyS0 console=tty0 rootfstype=ext4
EOCONF

echo "=== 2. Configuring Rootfs ==="
sudo cp -a "$ROOTFS"/* "$WORK_DIR/rootfs/"
# Remove redundant boot directory inside rootfs
sudo rm -rf "$WORK_DIR/rootfs/boot"/*

# Fix root password
sudo sed -i 's/^root:[^:]*:/root::/' "$WORK_DIR/rootfs/etc/shadow"

# Configure inittab for direct serial autologin
sudo tee "$WORK_DIR/rootfs/etc/inittab" > /dev/null << 'EOINIT'
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

# Auto-shell on serial port for QEMU / automated testing
ttyS0::respawn:/bin/sh
tty1::respawn:/bin/sh

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOINIT

# Greeting
sudo mkdir -p "$WORK_DIR/rootfs/etc/profile.d"
sudo tee "$WORK_DIR/rootfs/etc/profile.d/rim-greeting.sh" > /dev/null << 'EOPROFILE'
echo "======================================================"
echo " Welcome to Alpine Linux synthesized by RIM storage! "
echo " Kernel: $(uname -r) on $(uname -m)"
echo " Rootfs: $(mount | grep ' / ' | awk '{print $1}')"
echo "======================================================"
# Add demo.toml for RIM Inception
sudo tee "$WORK_DIR/rootfs/root/demo.toml" > /dev/null << 'EODEMO'
[disk]
size = "32M"
guid = "11111111-2222-3333-4444-555555555555"

[[partitions]]
name = "NESTED_EXT4"
filesystem = "ext4"
size = "20M"
label = "NESTED"
guid = "22222222-3333-4444-5555-666666666666"
uuid = "33333333-4444-5555-6666-777777777777"
EODEMO

RIM_BIN="$SCRIPT_DIR/../../../target/x86_64-unknown-linux-musl/release/rim"
if [ -f "$RIM_BIN" ]; then
    echo "Installing native rim binary into guest rootfs /usr/bin/rim..."
    sudo cp -f "$RIM_BIN" "$WORK_DIR/rootfs/usr/bin/rim"
    sudo strip "$WORK_DIR/rootfs/usr/bin/rim" || true
    sudo chmod 755 "$WORK_DIR/rootfs/usr/bin/rim"
fi

echo "=== 3. Creating TAR archive ==="
cd "$WORK_DIR"
sudo tar -cf "alpine_payload.tar" esp rootfs
sudo gzip -k -f "alpine_payload.tar"
sudo chown mk:mk alpine_payload.tar*

echo "Payload packaged successfully: $(ls -lh alpine_payload.tar)"