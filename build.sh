#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Detect tools - use installed paths
NASM="${NASM:-/tmp/nasm-extract/usr/bin/nasm}"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
LD_LLD="${LD_LLD:-$HOME/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld/ld.lld}"
OBJCOPY="${OBJCOPY:-objcopy}"

echo "=== DeltaOS Build ==="
echo "NASM:    $NASM"
echo "CARGO:   $CARGO"
echo "LD:      $LD_LLD"
echo "OBJCOPY: $OBJCOPY"
echo ""

# Step 1: Clean and create build dir
echo "[1/6] Preparing build directory..."
rm -rf build
mkdir -p build

# Step 2: Assemble NASM files (64-bit ELF objects)
echo "[2/6] Assembling boot code..."
$NASM -f elf64 boot/boot.asm -o build/boot.o
$NASM -f elf64 boot/long_mode.asm -o build/long_mode.o
$NASM -f elf64 src/sched/context_switch.asm -o build/context_switch.o

# Step 3: Assemble MBR (raw 16-bit binary)
echo "[3/6] Assembling MBR bootloader..."
$NASM -f bin boot/mbr.asm -o build/mbr.bin

# Step 4: Build Rust kernel
echo "[4/6] Compiling Rust kernel..."
$CARGO +nightly build --target x86_64-unknown-none --release
cp target/x86_64-unknown-none/release/libdeltaos_kernel.a build/libdeltaos_kernel.a

# Step 5: Link ELF
echo "[5/6] Linking kernel..."
$LD_LLD -T linker.ld -nostdlib --gc-sections -z max-page-size=0x1000 -o build/deltaos.elf \
    build/boot.o \
    build/long_mode.o \
    build/context_switch.o \
    build/libdeltaos_kernel.a

# Create flat binary
$OBJCOPY -O binary build/deltaos.elf build/deltaos.bin

# Step 6: Build disk image
echo "[6/6] Creating disk image..."
python3 build_img.py

echo ""
echo "=== Build Complete ==="
echo "ELF kernel:  build/deltaos.elf"
echo "Flat binary:  build/deltaos.bin"
echo "Disk image:   build/deltaos.img"
echo "MBR boot:     build/mbr.bin"
echo ""
echo "To run with QEMU:"
echo "  ./qemu/run.sh       (recommended - multiboot2)"
echo "  ./qemu/run-img.sh   (MBR bootloader)"
