#!/usr/bin/env bash
set -euo pipefail

# --- Params ---
FS="${1:-fat32}"                     # fat32 | exfat
SRC="${2:-./rimfs/test_data}"        # répertoire source
IMG="${3:-test.img}"
MNT="${4:-./mnt_test}"

# --- Timer utils (WSL-friendly) ---
now_ns() { date +%s%N; }
dur_ms() { awk "BEGIN{printf \"%.3f\", ($2-$1)/1e6}"; }  # start_ns end_ns
print_time() { printf "%-18s : %8.3f ms\n" "$1" "$2"; }

cleanup() {
  set +e
  sudo umount "$MNT" 2>/dev/null || true
  if [[ -n "${LOOPDEV:-}" && "$LOOPDEV" != "" ]]; then
    sudo losetup -d "$LOOPDEV" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# --- Prep ---
[[ -d "$SRC" ]] || { echo "Source not found: $SRC"; exit 1; }
umount -q "$MNT" 2>/dev/null || true
rm -rf "$MNT" "$IMG"
mkdir -p "$MNT"

# --- Image 32 MiB (forcé) ---
SIZE_MB=32
echo "[*] Image size FORCED = ${SIZE_MB} MiB (fs=$FS)"
truncate -s ${SIZE_MB}M "$IMG"

# --- Loop device unique pour tout le run ---
LOOPDEV=$(sudo losetup -f --show "$IMG")

# --- FORMAT ---
t0=$(now_ns)
case "$FS" in
  fat32) mkfs.vfat -F 32 "$LOOPDEV" >/dev/null ;;
  exfat) mkfs.exfat "$LOOPDEV"      >/dev/null ;;
  *) echo "Usage: $0 {fat32|exfat} [src_dir] [img] [mnt_dir]"; exit 1 ;;
esac
t1=$(now_ns)
FORMAT_MS=$(dur_ms "$t0" "$t1")

# --- Mount helpers ---
mount_rw() {
  if [[ "$FS" == "exfat" ]]; then
    if sudo mount -t exfat -o uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT" 2>/dev/null; then
      return 0
    else
      sudo /sbin/mount.exfat-fuse -o uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT"
    fi
  else
    sudo mount -t vfat -o uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT"
  fi
}
mount_ro() {
  sudo umount "$MNT" 2>/dev/null || true
  if [[ "$FS" == "exfat" ]]; then
    if sudo mount -t exfat -o ro,uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT" 2>/dev/null; then
      return 0
    else
      sudo /sbin/mount.exfat-fuse -o ro,uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT"
    fi
  else
    sudo mount -t vfat -o ro,uid=$(id -u),gid=$(id -g),umask=022 "$LOOPDEV" "$MNT"
  fi
}

mount_rw

# ---------- Helpers d'escape (FAT32) ----------
is_reserved_vfat() {
  local upper="${1^^}"
  case "$upper" in CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]) return 0;; esac
  return 1
}
sanitize_component() {
  local comp="$1" out
  if command -v iconv >/dev/null 2>&1; then
    out=$(printf '%s' "$comp" | iconv -f utf-8 -t ascii//TRANSLIT 2>/dev/null || printf '%s' "$comp")
  else
    out="$comp"
  fi
  out=$(printf '%s' "$out" | sed 's/[^A-Za-z0-9._-]/_/g')
  is_reserved_vfat "$out" && out="${out}_"
  [[ -z "$out" ]] && out="_"
  printf '%s' "$out"
}
safe_path_from_rel() {
  local rel="$1"; IFS='/' read -r -a parts <<< "$rel"
  local out="" comp safe
  for comp in "${parts[@]}"; do
    [[ -z "$comp" || "$comp" == "." ]] && continue
    safe=$(sanitize_component "$comp")
    [[ -z "$out" ]] && out="$safe" || out="$out/$safe"
  done
  printf '%s' "$out"
}
# ----------------------------------------------

# --- COPY / INJECT ---
t2=$(now_ns)
if [[ "$FS" == "exfat" ]]; then
  if command -v rsync >/dev/null 2>&1; then
    rsync -a --no-o --no-g --inplace --whole-file --delete --info=progress2 "$SRC"/ "$MNT"/
  else
    cp -r "$SRC"/. "$MNT"/
  fi
else
  echo "[*] Copy with FAT32-safe escaping…"
  MAP="${IMG}.map.txt"; : > "$MAP"

  # Dossiers
  while IFS= read -r -d '' d; do
    rel="${d#"$SRC"/}"; safe_rel="$(safe_path_from_rel "$rel")"
    mkdir -p "$MNT/$safe_rel"
  done < <(find "$SRC" -type d -print0)

  # Fichiers
  while IFS= read -r -d '' f; do
    rel="${f#"$SRC"/}"; safe_rel="$(safe_path_from_rel "$rel")"
    mkdir -p "$MNT/$(dirname "$safe_rel")"
    # collision -> suffixe hash court
    if [[ -e "$MNT/$safe_rel" ]]; then
      hash=$(printf '%s' "$rel" | md5sum | awk '{print $1}' | cut -c1-8)
      base="$(basename "$safe_rel")"; dir="$(dirname "$safe_rel")"; ext=""
      if [[ "$base" == *.* ]]; then ext=".${base##*.}"; base="${base%.*}"; fi
      safe_rel="$dir/${base}_$hash$ext"
    fi
    echo "$rel -> $safe_rel" >> "$MAP"
    cp "$f" "$MNT/$safe_rel"
  done < <(find "$SRC" -type f -print0)
fi
sync
t3=$(now_ns)
INJECT_MS=$(dur_ms "$t2" "$t3")
sudo umount "$MNT"

# --- CHECK (fsck) ---
t4=$(now_ns)
if [[ "$FS" == "fat32" ]]; then
  fsck.fat -n -v "$LOOPDEV" >/dev/null || true
else
  fsck.exfat -n -v "$LOOPDEV" >/dev/null || true
fi
t5=$(now_ns)
CHECK_MS=$(dur_ms "$t4" "$t5")

# --- Traversal (FIND) ---
mount_ro
echo "[*] Counting files:"
FILECOUNT=$(find "$MNT" -type f | wc -l)
echo "$FILECOUNT files"
echo "[*] Timing traversal:"
t6=$(now_ns)
find "$MNT" -type f >/dev/null
t7=$(now_ns)
TRAV_MS=$(dur_ms "$t6" "$t7")
sudo umount "$MNT"

# --- Summary ---
echo "---- Timing Summary ----"
print_time "Format"        "$FORMAT_MS"
print_time "Inject/Copy"   "$INJECT_MS"
print_time "Check (fsck)"  "$CHECK_MS"
print_time "Traversal"     "$TRAV_MS"
TOTAL_MS=$(awk -v a="$FORMAT_MS" -v b="$INJECT_MS" -v c="$CHECK_MS" -v d="$TRAV_MS" 'BEGIN{printf "%.3f", a+b+c+d}')
print_time "Total (sum)"   "$TOTAL_MS"
echo "[*] Done → $IMG (${SIZE_MB} MiB)"
