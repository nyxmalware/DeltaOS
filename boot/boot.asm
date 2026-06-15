[bits 32]

EXTERN long_mode_start

section .multiboot
align 4
    ; Jump over the multiboot header to the real code
    ; This ensures that if the binary is loaded at 0x100000 and execution
    ; starts at 0x100000, we jump to the actual entry point at 0x101000
    jmp dword _start_real
    nop
    nop
    ; Multiboot1 header (must be within first 8192 bytes)
    dd 0x1BADB002    ; magic
    dd 0x00000003    ; flags: align modules + memory info
    dd -(0x1BADB002 + 0x00000003)  ; checksum
    ; Multiboot2 header (also include for compatibility)
    dd 0xE85250D6    ; magic
    dd 0             ; architecture
    dd mb2_end - mb2_start  ; header length
    dd -(0xE85250D6 + 0 + (mb2_end - mb2_start))  ; checksum
mb2_start:
    dw 0, 0          ; end tag type=0, flags=0
    dd 8             ; end tag size=8
mb2_end:

; PVH .note section removed - use multiboot2 only for QEMU -kernel boot

section .bss
align 4096
p4_table:
    resb 4096
p3_table:
    resb 4096
p2_table:
    resb 4096
stack_bottom:
    resb 16384
stack_top:

section .text.asm

global _start_real

_start_real:
    ; Write 'A' directly to VGA memory at 0xB8000 for debug
    mov dword [0xB8000], 0x0F410F4D  ; 'M' and 'A' in white on black

    ; Debug: send 'A' to COM1 serial port
    mov dx, 0x3FD
.wait_tx_A:
    in al, dx
    test al, 0x20
    jz .wait_tx_A
    mov dx, 0x3F8
    mov al, 'A'
    out dx, al

    mov esp, stack_top

    ; Check if paging is already enabled (PVH boot sets up paging)
    mov eax, cr0
    test eax, 0x80000000      ; Test CR0.PG bit
    jnz .paging_already_on    ; If paging is on, skip setup

    call setup_page_tables

    ; Debug: send 'B' after page tables set up
    mov dx, 0x3FD
.wait_tx_B:
    in al, dx
    test al, 0x20
    jz .wait_tx_B
    mov dx, 0x3F8
    mov al, 'B'
    out dx, al

    call enable_paging

    ; Debug: send 'C' after paging enabled
    mov dx, 0x3FD
.wait_tx_C:
    in al, dx
    test al, 0x20
    jz .wait_tx_C
    mov dx, 0x3F8
    mov al, 'C'
    out dx, al

.paging_already_on:
    lgdt [gdt64.pointer]

    ; Debug: send 'D' before long mode jump
    mov dx, 0x3FD
.wait_tx_D:
    in al, dx
    test al, 0x20
    jz .wait_tx_D
    mov dx, 0x3F8
    mov al, 'D'
    out dx, al

    jmp gdt64.code_seg:long_mode_start

    hlt

setup_page_tables:
    mov ecx, 0x1000
    mov edi, p4_table
    xor eax, eax
    rep stosd

    mov ecx, 0x1000
    mov edi, p3_table
    xor eax, eax
    rep stosd

    mov ecx, 0x1000
    mov edi, p2_table
    xor eax, eax
    rep stosd

    mov eax, p3_table
    or eax, 0b11
    mov dword [p4_table], eax

    mov eax, p2_table
    or eax, 0b11
    mov dword [p3_table], eax

    mov ecx, 0
.map_p2_table:
    mov eax, 0x200000
    mul ecx
    or eax, 0b10000011
    mov [p2_table + ecx * 8], eax

    inc ecx
    cmp ecx, 512
    jne .map_p2_table

    ret

enable_paging:
    mov eax, p4_table
    mov cr3, eax

    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax

    ret

section .rodata.asm
gdt64:
    dq 0
.code_seg: equ $ - gdt64
    dq (1<<41) | (1<<43) | (1<<44) | (1<<47) | (1<<53)
.data_seg: equ $ - gdt64
    dq (1<<44) | (1<<47) | (1<<41)
.pointer:
    dw $ - gdt64 - 1
    dq gdt64
