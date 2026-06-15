[bits 16]
[org 0x7C00]

; DeltaOS MBR Bootloader for VirtualBox/QEMU
; Loads kernel from disk to 0x100000 and boots it

KERNEL_SECTORS equ 0

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    mov [boot_drv], dl

    ; Debug: Initialize COM1 and send 'M' to serial
    mov dx, 0x3F9       ; IER
    mov al, 0x00
    out dx, al          ; Disable interrupts
    mov dx, 0x3FB       ; LCR
    mov al, 0x80
    out dx, al          ; Enable DLAB
    mov dx, 0x3F8       ; DLL
    mov al, 0x01
    out dx, al          ; Divisor lo = 1 (115200 baud)
    mov dx, 0x3F9       ; DLM
    mov al, 0x00
    out dx, al          ; Divisor hi = 0
    mov dx, 0x3FA       ; FCR
    mov al, 0xC7
    out dx, al          ; Enable FIFO, clear, 14-byte threshold
    mov dx, 0x3FB       ; LCR
    mov al, 0x03
    out dx, al          ; 8N1, DLAB=0
    mov dx, 0x3FC       ; MCR
    mov al, 0x0B
    out dx, al          ; RTS/DSR set
    ; Send 'M' to COM1
    mov dx, 0x3FD
.wait_tx:
    in al, dx
    test al, 0x20
    jz .wait_tx
    mov dx, 0x3F8
    mov al, 'M'
    out dx, al

    ; Enable A20 via fast method (port 0x92)
    in al, 0x92
    or al, 2
    and al, 0xFE
    out 0x92, al

    ; Set up unreal mode (FS with 4GB limit for writing above 1MB)
    cli
    lgdt [ugdt_d]
    mov eax, cr0
    inc eax
    mov cr0, eax
    mov bx, 8
    mov fs, bx
    dec eax
    mov cr0, eax
    sti

    ; Prepare kernel load
    mov dword [tgt], 0x100000
    mov dword [clba], 1
    mov word [sleft], KERNEL_SECTORS

.rl:
    cmp word [sleft], 0
    je .ldone
    mov ax, [sleft]
    cmp ax, 32
    jle .ssz
    mov ax, 32
.ssz:
    mov [dcnt], ax
    mov eax, [clba]
    mov [dlba], eax
    mov ah, 0x42
    mov dl, [boot_drv]
    mov si, dap
    int 0x13
    jc .derr
    pusha
    movzx ecx, word [dcnt]
    shl ecx, 9
    shr ecx, 2
    mov esi, 0x80000
    mov edi, [tgt]
.cpy:
    mov eax, [esi]
    mov fs:[edi], eax
    add esi, 4
    add edi, 4
    dec ecx
    jnz .cpy
    mov [tgt], edi
    popa
    movzx eax, word [dcnt]
    add [clba], eax
    sub [sleft], ax
    jmp .rl

.ldone:
    ; Debug: Send 'K' to serial (kernel loaded)
    mov dx, 0x3F8 + 5
.wait_tx2:
    in al, dx
    test al, 0x20
    jz .wait_tx2
    mov dx, 0x3F8
    mov al, 'K'
    out dx, al

    ; Print 'K' to VGA to indicate kernel loaded
    mov al, 'K'
    mov ah, 0x0E
    int 0x10

    ; Enter protected mode
    cli
    lgdt [pgdt_d]
    mov eax, cr0
    inc eax
    mov cr0, eax
    ; Print 'P' to indicate protected mode entered (before segment switch)
    ; Set data segments in 16-bit protected mode
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov esp, 0x90000

    ; Debug: Send 'J' before far jump (in protected mode)
    mov dx, 0x3FD
.wait_tx3:
    in al, dx
    test al, 0x20
    jz .wait_tx3
    mov dx, 0x3F8
    mov al, 'J'
    out dx, al

    ; Far jump to kernel at 0x100000 (which has a jmp to 0x101000)
    ; Using retf method for proper 32-bit transition
    push word 0x08
    push dword 0x100000
    db 0x66, 0xCB          ; o32 retf = far jump to 0x08:0x100000

.derr:
    mov si, emsg
    mov ah, 0x0E
.pr: lodsb
    test al, al
    jz .ht
    int 0x10
    jmp .pr
.ht: cli
    hlt

; ---- Data ----
boot_drv:  db 0
tgt:       dd 0x100000
clba:      dd 1
sleft:     dw 0
emsg:      db 'Disk error!', 0

; DAP for INT 13h AH=42h
dap:
    db 0x10, 0
dcnt:
    dw 32
    dw 0          ; offset
    dw 0x8000     ; segment (0x8000:0 = 0x80000)
dlba:
    dd 1
    dd 0

; Unreal mode GDT (4GB data segment)
ugdt:
    dq 0
    dq 0x00CF92000000FFFF
ugdt_d:
    dw $ - ugdt - 1
    dd ugdt

; Protected mode GDT
pgdt:
    dq 0
    dw 0xFFFF, 0
    db 0, 10011010b, 11001111b, 0  ; Code 32-bit (0x08)
    dw 0xFFFF, 0
    db 0, 10010010b, 11001111b, 0  ; Data (0x10)
pgdt_d:
    dw $ - pgdt - 1
    dd pgdt

times 510 - ($ - $$) db 0
dw 0xAA55
