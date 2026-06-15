OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start_real)

PHDRS
{
    text    PT_LOAD FLAGS(5);
    rodata  PT_LOAD FLAGS(4);
    data    PT_LOAD FLAGS(6);
    bss     PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = 0x100000;

    _kernel_start = .;

    .multiboot ALIGN(4K) : {
        KEEP(*(.multiboot))
    } :text

    .note.gnu.build ALIGN(4) : {
        KEEP(*(.note.gnu.build))
    } :text

    .text ALIGN(4K) : {
        *(.text.asm)
        *(.text.asm.*)
        *(.text._start)
        *(.text .text.*)
    } :text

    .rodata ALIGN(4K) : {
        *(.rodata.asm)
        *(.rodata.asm.*)
        *(.rodata .rodata.*)
    } :rodata

    .data ALIGN(4K) : {
        *(.data .data.*)
        *(.data.asm)
        *(.data.asm.*)
    } :data

    .bss ALIGN(4K) : {
        *(COMMON)
        *(.bss .bss.*)
        . = ALIGN(4K);
        _kernel_stack_bottom = .;
        . += 0x40000;
        _kernel_stack_top = .;
    } :bss

    . = ALIGN(4K);
    _kernel_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.note.GNU-stack)
        *(.note.ABI-tag)
        *(.eh_frame*)
    }
}
