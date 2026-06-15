//! Interrupt handling
//!
//! Manages interrupt service routines (ISRs) for:
//! - CPU exceptions (divide error, page fault, GP, etc.)
//! - Hardware interrupts (keyboard, timer)
//! - System calls (int 0x80)
//! - APIC spurious interrupts (vector 0xFF)

use crate::kprint;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub fn init() {}

// ── Keyboard Buffer ─────────────────────────────────────────────────

static mut KEYBOARD_BUFFER: [u8; 256] = [0; 256];
static mut KEYBOARD_HEAD: usize = 0;
static mut KEYBOARD_TAIL: usize = 0;

pub fn keyboard_has_data() -> bool {
    unsafe { KEYBOARD_HEAD != KEYBOARD_TAIL }
}

pub fn keyboard_read_char() -> u8 {
    unsafe {
        if KEYBOARD_HEAD == KEYBOARD_TAIL {
            return 0;
        }
        let ch = KEYBOARD_BUFFER[KEYBOARD_TAIL];
        KEYBOARD_TAIL = (KEYBOARD_TAIL + 1) % 256;
        ch
    }
}

static mut SHIFT_PRESSED: bool = false;
static mut CAPS_LOCK: bool = false;

const SCANCODE_TO_ASCII: &[u8; 128] = &[
    0, 27, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', 8,
    b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n',
    0, b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`',
    0, b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', 0,
    b'*', 0, b' ',
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, b'-', 0, 0, 0, b'+', 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const SCANCODE_TO_ASCII_SHIFT: &[u8; 128] = &[
    0, 27, b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')', b'_', b'+', 8,
    b'\t', b'Q', b'W', b'E', b'R', b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', b'\n',
    0, b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':', b'"', b'~',
    0, b'|', b'Z', b'X', b'C', b'V', b'B', b'N', b'M', b'<', b'>', b'?', 0,
    b'*', 0, b' ',
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, b'-', 0, 0, 0, b'+', 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

fn handle_keyboard_scancode(scancode: u8) {
    let released = (scancode & 0x80) != 0;
    let key = scancode & 0x7F;

    match key {
        0x2A | 0x36 => {
            unsafe { SHIFT_PRESSED = !released; }
            return;
        }
        0x3A => {
            if !released {
                unsafe { CAPS_LOCK = !CAPS_LOCK; }
            }
            return;
        }
        _ => {}
    }

    if released {
        return;
    }

    if key < 128 {
        let ch = unsafe {
            if SHIFT_PRESSED {
                SCANCODE_TO_ASCII_SHIFT[key as usize]
            } else {
                SCANCODE_TO_ASCII[key as usize]
            }
        };

        if ch != 0 {
            unsafe {
                let next_head = (KEYBOARD_HEAD + 1) % 256;
                if next_head != KEYBOARD_TAIL {
                    KEYBOARD_BUFFER[KEYBOARD_HEAD] = ch;
                    KEYBOARD_HEAD = next_head;
                }
            }
        }
    }
}

// ── Main ISR Handler ────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn rust_isr_handler(ctx: &mut InterruptContext) {
    match ctx.vector {
        // CPU Exceptions
        0 => kprint!("[EXC] #DE Divide Error at RIP={:#X}\n", ctx.rip),
        6 => kprint!("[EXC] #UD Invalid Opcode at RIP={:#X}\n", ctx.rip),
        8 => {
            kprint!("[EXC] #DF Double Fault!\n");
            loop { unsafe { core::arch::asm!("hlt") } }
        }
        13 => kprint!("[EXC] #GP General Protection at RIP={:#X} err={:#X}\n", ctx.rip, ctx.error_code),
        14 => {
            let fault_addr: u64;
            unsafe {
                core::arch::asm!("mov {}, cr2", out(reg) fault_addr, options(nomem, nostack));
            }
            let present = (ctx.error_code & 1) != 0;
            let write = (ctx.error_code & 2) != 0;
            let user = (ctx.error_code & 4) != 0;
            kprint!("[EXC] #PF at RIP={:#X} addr={:#X} P={} W={} U={}\n", ctx.rip, fault_addr, present, write, user);
        }

        // IRQ 0 (vector 32) - Timer interrupt
        32 => {
            // Notify timer subsystem
            crate::timer::on_timer_tick();

            // Call scheduler tick for preemptive scheduling
            // The scheduler will decide if a context switch is needed
            unsafe {
                if let Some(ref mut scheduler) = crate::KERNEL_SCHEDULER {
                    scheduler.tick();
                }
            }

            // Send EOI
            send_eoi(0);
        }

        // IRQ 1 (vector 33) - Keyboard
        33 => {
            let scancode = crate::inb(0x60);
            handle_keyboard_scancode(scancode);
            send_eoi(1);
        }

        // IRQ 7 (vector 39) - Spurious (PIC)
        39 => {
            // Check if this is a real IRQ7 or spurious
            // Spurious IRQs don't need EOI for the slave PIC
        }

        // IRQ 15 (vector 47) - Spurious (PIC)
        47 => {
            // Spurious IRQ from slave PIC
        }

        // Syscall (int 0x80 = vector 128)
        128 => {
            let result = crate::syscall::rust_syscall_handler(
                ctx.rax,
                ctx.rdi,
                ctx.rsi,
                ctx.rdx,
                ctx.r10,
            );
            ctx.rax = result as u64;
        }

        // APIC Spurious interrupt (vector 0xFF = 255)
        255 => {
            // LAPIC spurious interrupt - no EOI needed
        }

        _ => {
            if ctx.vector < 32 {
                kprint!("[EXC] Vector {} at RIP={:#X}\n", ctx.vector, ctx.rip);
            }
        }
    }

    // Send EOI for hardware interrupts (IRQ 0-15 -> vectors 32-47)
    // Note: IRQ 0 and 1 already handled above with explicit EOI
    if ctx.vector >= 34 && ctx.vector < 48 {
        send_eoi((ctx.vector - 32) as u8);
    }
}

fn send_eoi(irq: u8) {
    // If APIC is available and initialized, use LAPIC EOI
    if crate::apic::LocalApic::is_using_apic() {
        crate::apic::LocalApic::eoi();
    } else {
        // Legacy PIC EOI
        if irq >= 8 {
            crate::outb(0xA0, 0x20);
        }
        crate::outb(0x20, 0x20);
    }
}

// ── Timer ISR handler (called from dedicated timer_isr ASM stub) ────

/// This is called from the timer_isr assembly stub with a pointer to
/// the saved register state on the stack. It performs the timer tick
/// and returns the RSP to use (same or new task's).
#[no_mangle]
pub extern "C" fn rust_timer_handler(saved_state_ptr: u64) -> u64 {
    // Notify timer subsystem
    crate::timer::on_timer_tick();

    // Send EOI first so we can receive more interrupts
    send_eoi(0);

    // Call scheduler and get the RSP for the next (or current) task
    unsafe {
        if let Some(ref mut scheduler) = crate::KERNEL_SCHEDULER {
            if scheduler.is_preemption_enabled() {
                scheduler.preempt_schedule(saved_state_ptr)
            } else {
                saved_state_ptr
            }
        } else {
            saved_state_ptr
        }
    }
}

// ── APIC EOI handler (minimal) ──────────────────────────────────────

#[no_mangle]
pub extern "C" fn rust_apic_eoi_handler() {
    crate::apic::LocalApic::eoi();
}
