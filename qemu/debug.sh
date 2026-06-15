#!/bin/bash
# DeltaOS QEMU Debug Mode
# Starts QEMU with GDB server on port 1234

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ELF_PATH="$SCRIPT_DIR/../build/deltaos.elf"
QEMU="qemu-system-x86_64"

if [ ! -f "$ELF_PATH" ]; then
    echo "Error: Kernel ELF not found at $ELF_PATH"
    exit 1
fi

echo "=== DeltaOS QEMU Debug Mode ==="
echo "Kernel: $ELF_PATH"
echo "GDB server: tcp::1234"
echo ""
echo "Connect with: gdb $ELF_PATH -ex 'target remote :1234'"
echo ""

exec $QEMU \
    -kernel "$ELF_PATH" \
    -m 128M \
    -cpu qemu64 \
    -smp 1 \
    -no-reboot \
    -no-shutdown \
    -vga std \
    -serial stdio \
    -S -gdb tcp::1234
