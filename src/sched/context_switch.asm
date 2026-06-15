[bits 64]

section .text.asm

; ── Cooperative context switch (callee-saved registers only) ─────────
; Used by yield() and block/unblock operations.
; rdi = pointer to current SavedRegisters (7 qwords: rbx,rbp,r12-r15,rsp)
; rsi = pointer to next SavedRegisters

global context_switch

context_switch:
    ; Save callee-saved registers to current task
    mov [rdi + 0x00], rbx
    mov [rdi + 0x08], rbp
    mov [rdi + 0x10], r12
    mov [rdi + 0x18], r13
    mov [rdi + 0x20], r14
    mov [rdi + 0x28], r15
    mov [rdi + 0x30], rsp

    ; Load callee-saved registers from next task
    mov rbx, [rsi + 0x00]
    mov rbp, [rsi + 0x08]
    mov r12, [rsi + 0x10]
    mov r13, [rsi + 0x18]
    mov r14, [rsi + 0x20]
    mov r15, [rsi + 0x28]
    mov rsp, [rsi + 0x30]

    ret

; ── Preemptive context switch (full register save/restore) ───────────
; Called from timer ISR handler after deciding to switch tasks.
; The interrupt frame (pushed by CPU + ISR stub) is already on the stack.
;
; rdi = pointer to variable that holds current task's kernel RSP
;       (we save current RSP there, then load new RSP from rsi)
; rsi = the new task's kernel RSP (points to saved interrupt frame on its stack)

global preempt_switch

preempt_switch:
    ; Save current RSP into current task's saved_rsp pointer
    mov [rdi], rsp

    ; Load next task's RSP
    mov rsp, rsi

    ; The stack now points to the next task's saved state.
    ; The caller (ISR handler) will pop registers and iretq.
    ret

; ── Timer ISR stub for preemptive scheduling ─────────────────────────
; This is the dedicated entry point for the timer interrupt (vector 0x20).
; It saves all general-purpose registers, calls the Rust handler,
; and then handles potential context switching.
;
; The key insight: if the scheduler decides to switch tasks, it will
; change RSP to point to the new task's saved register state on its stack.
; Then when we pop registers and iretq, we resume the new task.

global timer_isr
extern rust_timer_handler

timer_isr:
    ; Save all general-purpose registers
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; Save DS (and other segment registers if needed)
    xor rax, rax
    mov ax, ds
    push rax

    ; Load kernel data segment
    mov ax, 0x10
    mov ds, ax
    mov es, ax

    ; Call Rust timer handler with pointer to saved state
    ; The handler may change the stack pointer if a context switch is needed
    mov rdi, rsp
    call rust_timer_handler

    ; The handler returns the RSP to use:
    ; - If no switch needed: returns the same RSP we passed
    ; - If switch needed: returns the new task's RSP
    mov rsp, rax

    ; Restore segment registers
    pop rax
    mov ds, ax
    mov es, ax

    ; Restore general-purpose registers
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    ; Send EOI (the Rust handler already sent it for APIC,
    ; but for PIC we need to do it here)
    ; Actually, EOI is handled in the Rust handler

    iretq

; ── APIC Spurious Interrupt Handler ──────────────────────────────────
; Vector 0xFF - spurious interrupts don't need EOI for APIC

global spurious_isr

spurious_isr:
    ; Spurious interrupt - no action needed
    ; For APIC: spurious interrupts do NOT need EOI
    iretq

; ── ISR stub for APIC-specific interrupts ────────────────────────────
; Generic ISR that sends APIC EOI

global apic_eoi_isr
extern rust_apic_eoi_handler

apic_eoi_isr:
    push rax
    mov rax, 1   ; indicate APIC EOI needed
    ; Call a minimal handler
    call rust_apic_eoi_handler
    pop rax
    iretq
