#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ISO_PATH="$SCRIPT_DIR/../build/deltaos.iso"
QEMU="qemu-system-x86_64"

if ! command -v $QEMU &>/dev/null; then
    echo "Error: $QEMU not found. Please install qemu-system-x86."
    exit 1
fi

if [ ! -f "$ISO_PATH" ]; then
    echo "Error: ISO not found at $ISO_PATH"
    exit 1
fi

echo "=== DeltaOS QEMU Boot (ISO) ==="
exec $QEMU \
    -cdrom "$ISO_PATH" \
    -m 128M \
    -cpu qemu64 \
    -smp 1 \
    -no-reboot \
    -no-shutdown \
    -vga std \
    -serial stdio
