@echo off
echo === DeltaOS QEMU Boot (Disk Image) ===
qemu-system-x86_64 -drive file=..\build\deltaos.img,format=raw,if=ide -m 128M -cpu qemu64 -smp 1 -no-reboot -no-shutdown -vga std -serial stdio
pause
