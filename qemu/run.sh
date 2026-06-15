#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ELF_PATH="$SCRIPT_DIR/../build/deltaos.elf"
QEMU="qemu-system-x86_64"

if ! command -v $QEMU &>/dev/null; then
    echo "Error: $QEMU not found. Please install qemu-system-x86."
    exit 1
fi

if [ ! -f "$ELF_PATH" ]; then
    echo "Error: Kernel ELF not found at $ELF_PATH"
    exit 1
fi

echo "=== DeltaOS QEMU Boot (multiboot2) ==="
exec $QEMU \
    -kernel "$ELF_PATH" \
    -m 128M \
    -cpu qemu64 \
    -smp 1 \
    -no-reboot \
    -no-shutdown \
    -vga std \
    -serial stdio
