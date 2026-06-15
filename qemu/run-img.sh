#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMG_PATH="$SCRIPT_DIR/../build/deltaos.img"
QEMU="qemu-system-x86_64"

if ! command -v $QEMU &>/dev/null; then
    echo "Error: $QEMU not found. Please install qemu-system-x86."
    exit 1
fi

if [ ! -f "$IMG_PATH" ]; then
    echo "Error: Disk image not found at $IMG_PATH"
    exit 1
fi

echo "=== DeltaOS QEMU Boot (Disk Image) ==="
exec $QEMU \
    -drive file="$IMG_PATH",format=raw,if=ide \
    -m 128M \
    -cpu qemu64 \
    -smp 1 \
    -no-reboot \
    -no-shutdown \
    -vga std \
    -serial stdio
