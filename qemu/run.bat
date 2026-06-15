@echo off
echo === DeltaOS QEMU Boot (ELF, multiboot2) ===
qemu-system-x86_64 -kernel ..\build\deltaos.elf -m 128M -cpu qemu64 -smp 1 -no-reboot -no-shutdown -vga std -serial stdio
pause
