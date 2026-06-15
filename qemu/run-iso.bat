@echo off
echo === DeltaOS QEMU Boot (ISO) ===
qemu-system-x86_64 -cdrom ..\build\deltaos.iso -m 128M -cpu qemu64 -smp 1 -no-reboot -no-shutdown -vga std -serial stdio
pause
