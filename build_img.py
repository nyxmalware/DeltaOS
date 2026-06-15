#!/usr/bin/env python3
"""
Build a bootable IMG disk image for DeltaOS (VirtualBox/QEMU).

Creates a raw disk image with:
  - Sector 0: MBR bootloader (loads kernel to 0x100000)
  - Sectors 1+: Kernel binary (deltaos.bin)

The bootloader is patched with the actual kernel size in sectors.
"""
import struct
import sys
import os

SECTOR_SIZE = 512
IMG_SIZE_MB = 4  # 4MB disk image

def main():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    build_dir = os.path.join(base_dir, 'build')

    mbr_path = os.path.join(build_dir, 'mbr.bin')
    kernel_path = os.path.join(build_dir, 'deltaos.bin')
    output_path = os.path.join(build_dir, 'deltaos.img')

    # Read MBR bootloader
    with open(mbr_path, 'rb') as f:
        mbr = bytearray(f.read())
    assert len(mbr) == SECTOR_SIZE, f"MBR must be 512 bytes, got {len(mbr)}"

    # Read kernel binary
    with open(kernel_path, 'rb') as f:
        kernel = f.read()

    # Calculate kernel size in sectors
    kernel_sectors = (len(kernel) + SECTOR_SIZE - 1) // SECTOR_SIZE
    print(f"Kernel size: {len(kernel)} bytes ({kernel_sectors} sectors)")

    # Patch KERNEL_SECTORS in the MBR
    # The MBR has: 'mov word [sleft], KERNEL_SECTORS' which assembles as:
    # C7 06 F8 7C 00 00  (without 66h prefix, since it's a 16-bit mov word)
    # The '66 C7 06' patterns are for 'mov dword [tgt]' and 'mov dword [clba]'
    # We must find C7 06 WITHOUT a preceding 66h byte
    
    patched = False
    
    # Search for 'mov word [sleft], 0' = C7 06 XX XX 00 00
    # But skip if preceded by 0x66 (which makes it a mov dword)
    for i in range(len(mbr) - 5):
        if mbr[i] == 0xC7 and mbr[i+1] == 0x06 and mbr[i+4] == 0x00 and mbr[i+5] == 0x00:
            # Check that this is NOT part of a 66h-prefixed instruction
            if i > 0 and mbr[i-1] == 0x66:
                continue  # Skip, this is part of mov dword
            # This should be 'mov word [sleft], 0'
            struct.pack_into('<H', mbr, i + 4, kernel_sectors)
            patched = True
            print(f"Patched KERNEL_SECTORS={kernel_sectors} at MBR offset {i}")
            break
    
    if not patched:
        print("WARNING: Could not patch KERNEL_SECTORS in MBR!")
        print("The bootloader will try to load 0 sectors (kernel won't load).")
        # Dump some of the MBR for debugging
        for i in range(0, len(mbr), 16):
            hex_str = ' '.join(f'{b:02X}' for b in mbr[i:i+16])
            print(f"  {i:04X}: {hex_str}")

    # Create disk image
    img_size = IMG_SIZE_MB * 1024 * 1024  # 4MB
    img = bytearray(img_size)
    
    # Write MBR at sector 0
    img[0:SECTOR_SIZE] = mbr
    
    # Write kernel starting at sector 1
    kernel_offset = SECTOR_SIZE
    img[kernel_offset:kernel_offset + len(kernel)] = kernel
    
    # Write output
    with open(output_path, 'wb') as f:
        f.write(img)
    
    print(f"Created {output_path} ({IMG_SIZE_MB}MB)")
    print(f"  MBR bootloader: 512 bytes")
    print(f"  Kernel: {len(kernel)} bytes ({kernel_sectors} sectors)")
    print(f"  Total used: {SECTOR_SIZE + len(kernel)} bytes")

if __name__ == '__main__':
    main()
