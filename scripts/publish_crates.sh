#!/usr/bin/env bash
# Bash script to publish all RIM workspace crates to crates.io in topological order
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RIM_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

EXECUTE=false
DELAY_SECONDS=25
START_FROM=""

for arg in "$@"; do
    case "$arg" in
        --execute|-e)
            EXECUTE=true
            ;;
        --delay=*)
            DELAY_SECONDS="${arg#*=}"
            ;;
        --from=*)
            START_FROM="${arg#*=}"
            ;;
        --help|-h)
            echo "Usage: ./publish_crates.sh [--execute] [--delay=SECONDS] [--from=CRATE_NAME]"
            exit 0
            ;;
    esac
done

CRATES=(
    "rimio"
    "rimpart"
    "rimimg"
    "rimfs-core"
    "rimfs-fat"
    "rimfs-exfat"
    "rimfs-ext"
    "rimfs-ntfs"
    "rimfs-tar"
    "rimfs-zip"
    "rimfs-iso"
    "rimfs"
    "rimgen"
    "rimhost"
    "rimcli"
)

echo "================================================="
echo "       RIM Crates.io Automated Publisher         "
echo "================================================="
echo "Working Directory : $RIM_ROOT"
echo "Total Crates      : ${#CRATES[@]}"
if [ "$EXECUTE" = true ]; then
    echo "Mode              : LIVE PUBLISH (--execute)"
else
    echo "Mode              : DRY RUN (pass --execute to publish)"
fi
echo "Index Delay       : ${DELAY_SECONDS}s between crates"
if [ -n "$START_FROM" ]; then
    echo "Starting from     : $START_FROM"
fi
echo "================================================="
echo ""

STARTED=true
if [ -n "$START_FROM" ]; then
    STARTED=false
fi

TOTAL=${#CRATES[@]}
INDEX=0

cd "$RIM_ROOT"

for crate in "${CRATES[@]}"; do
    INDEX=$((INDEX + 1))
    PREFIX="[$INDEX/$TOTAL]"

    if [ "$STARTED" = false ]; then
        if [ "$crate" = "$START_FROM" ]; then
            STARTED=true
        else
            echo "$PREFIX Skipping $crate (waiting for $START_FROM)..."
            continue
        fi
    fi

    echo "$PREFIX Processing '$crate'..."

    if [ "$EXECUTE" = true ]; then
        echo "  -> Running: cargo publish -p $crate"
        set +e
        OUTPUT=$(cargo publish -p "$crate" 2>&1)
        STATUS=$?
        set -e
        echo "$OUTPUT"

        if [ $STATUS -ne 0 ]; then
            if echo "$OUTPUT" | grep -Eq "already uploaded|already exists"; then
                echo "  [OK] Crate $crate is already published at this version."
            else
                echo "  [ERROR] Failed to publish $crate!"
                exit 1
            fi
        else
            echo "  [OK] Successfully published $crate to crates.io!"
        fi

        if [ $INDEX -lt $TOTAL ]; then
            echo "  Waiting ${DELAY_SECONDS}s for crates.io index propagation..."
            sleep "$DELAY_SECONDS"
        fi
    else
        echo "  -> [Dry-Run] cargo publish -p $crate --dry-run"
        cargo publish -p "$crate" --dry-run || echo "  [NOTE] Requires prior dependencies to be published."
    fi
    echo ""
done

echo "================================================="
echo "Publication sequence completed successfully!"
echo "================================================="
