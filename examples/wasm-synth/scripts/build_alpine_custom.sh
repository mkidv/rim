#!/bin/bash
set -euo pipefail

ROOTFS="/tmp/alpine_rootfs"
sudo rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"
tar -xzf /tmp/alpine_prep/minirootfs.tar.gz -C "$ROOTFS"
sudo cp /etc/resolv.conf "$ROOTFS/etc/"

echo "Running apk update and install inside rootfs..."
sudo chroot "$ROOTFS" /bin/sh -c 'apk update && apk add --no-cache linux-virt mkinitfs'

echo "Generating ext4+virtio initramfs..."
KVER=$(ls "$ROOTFS/lib/modules" | head -n 1)
sudo chroot "$ROOTFS" /bin/sh -c "mkinitfs -F 'base virtio ext4' -k $KVER -o /boot/initramfs-virt"

echo "Boot files generated:"
ls -lh "$ROOTFS/boot/"