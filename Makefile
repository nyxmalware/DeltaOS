SRC_DIR     := src
BOOT_DIR    := boot
BUILD_DIR   := build

NASM        := nasm
CARGO       := cargo
LD          := ld.lld
OBJCOPY     := objcopy
QEMU        := qemu-system-x86_64

NASM_FLAGS  := -f elf64

KERNEL_BIN  := $(BUILD_DIR)/deltaos.bin
KERNEL_ELF  := $(BUILD_DIR)/deltaos.elf
ISO_DIR     := $(BUILD_DIR)/iso

ASM_SOURCES := $(BOOT_DIR)/boot.asm \
               $(BOOT_DIR)/long_mode.asm \
               $(SRC_DIR)/sched/context_switch.asm

ASM_OBJECTS := $(BUILD_DIR)/boot.o \
               $(BUILD_DIR)/long_mode.o \
               $(BUILD_DIR)/context_switch.o

.PHONY: all build-asm build-rust link run run-iso iso debug clean

all: link

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

build-asm: $(ASM_OBJECTS)

$(BUILD_DIR)/boot.o: $(BOOT_DIR)/boot.asm | $(BUILD_DIR)
	$(NASM) $(NASM_FLAGS) $< -o $@

$(BUILD_DIR)/long_mode.o: $(BOOT_DIR)/long_mode.asm | $(BUILD_DIR)
	$(NASM) $(NASM_FLAGS) $< -o $@

$(BUILD_DIR)/context_switch.o: $(SRC_DIR)/sched/context_switch.asm | $(BUILD_DIR)
	$(NASM) $(NASM_FLAGS) $< -o $@

build-rust: | $(BUILD_DIR)
	cargo +nightly build --target x86_64-unknown-none --release
	cp target/x86_64-unknown-none/release/libdeltaos_kernel.a $(BUILD_DIR)/libdeltaos_kernel.a

link: build-asm build-rust $(KERNEL_ELF) $(KERNEL_BIN)

$(KERNEL_ELF): $(ASM_OBJECTS) $(BUILD_DIR)/libdeltaos_kernel.a linker.ld | $(BUILD_DIR)
	$(LD) -T linker.ld -nostdlib --gc-sections -z max-page-size=0x1000 -o $@ \
		$(ASM_OBJECTS) \
		$(BUILD_DIR)/libdeltaos_kernel.a

$(KERNEL_BIN): $(KERNEL_ELF)
	$(OBJCOPY) -O binary $< $@

iso: $(KERNEL_ELF)
	mkdir -p $(ISO_DIR)/boot/grub
	cp $(KERNEL_ELF) $(ISO_DIR)/boot/deltaos.elf
	@echo 'set timeout=1' > $(ISO_DIR)/boot/grub/grub.cfg
	@echo 'set default=0' >> $(ISO_DIR)/boot/grub/grub.cfg
	@echo '' >> $(ISO_DIR)/boot/grub/grub.cfg
	@echo 'menuentry "DeltaOS" {' >> $(ISO_DIR)/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/deltaos.elf' >> $(ISO_DIR)/boot/grub/grub.cfg
	@echo '    boot' >> $(ISO_DIR)/boot/grub/grub.cfg
	@echo '}' >> $(ISO_DIR)/boot/grub/grub.cfg
	grub-mkrescue -o $(BUILD_DIR)/deltaos.iso $(ISO_DIR) 2>/dev/null || \
		xorriso -as mkisofs -R -J -c boot/boot.cat \
			-b boot/grub/i386-pc/eltorito.img \
			-no-emul-boot -boot-load-size 4 -boot-info-table \
			-o $(BUILD_DIR)/deltaos.iso $(ISO_DIR)

run: $(KERNEL_ELF)
	$(QEMU) \
		-kernel $(KERNEL_ELF) \
		-m 128M -cpu qemu64 -smp 1 \
		-no-reboot -serial stdio

run-iso: $(BUILD_DIR)/deltaos.iso
	$(QEMU) -cdrom $(BUILD_DIR)/deltaos.iso -m 128M -cpu qemu64 -smp 1 \
		-no-reboot -serial stdio

debug: $(KERNEL_ELF)
	$(QEMU) \
		-kernel $(KERNEL_ELF) \
		-m 128M -cpu qemu64 -smp 1 \
		-no-reboot -serial stdio \
		-S -gdb tcp::1234 &
	@echo "Connect: gdb $(KERNEL_ELF) -ex 'target remote :1234'"

clean:
	rm -rf $(BUILD_DIR) target
