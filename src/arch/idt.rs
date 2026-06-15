//! Interrupt Descriptor Table (IDT)
//!
//! Manages the IDT setup including:
//! - CPU exception handlers (vectors 0-31)
//! - Hardware interrupt handlers (vectors 32-47)
//! - System call gate (vector 0x80)
//! - APIC spurious handler (vector 0xFF)
//! - Dedicated timer ISR for preemptive scheduling

use core::mem;

const IDT_ENTRIES: usize = 256;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl Default for IdtEntry {
    fn default() -> Self {
        IdtEntry {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry {
    offset_low: 0,
    selector: 0,
    ist: 0,
    attributes: 0,
    offset_middle: 0,
    offset_high: 0,
    reserved: 0,
}; IDT_ENTRIES];

const IDT_PRESENT: u8 = 0x80;
const IDT_INTERRUPT_GATE: u8 = 0x0E;
const IDT_TRAP_GATE: u8 = 0x0F;
const IDT_DPL_KERNEL: u8 = 0x00;
const IDT_DPL_USER: u8 = 0x60;

// ISR stubs from assembly
extern "C" {
    static isr_stub_table: [u64; 49];
}

// Dedicated timer ISR stub from context_switch.asm
extern "C" {
    fn timer_isr();
    fn spurious_isr();
}

pub unsafe fn init() {
    for entry in IDT.iter_mut() {
        *entry = IdtEntry::default();
    }

    // CPU exception handlers (vectors 0-31)
    for i in 0..32 {
        let addr = isr_stub_table[i];
        set_handler(i, addr, IDT_PRESENT | IDT_INTERRUPT_GATE | IDT_DPL_KERNEL);
    }

    // Hardware interrupt handlers (vectors 32-47)
    // Vector 32 = IRQ 0 (Timer) - use dedicated timer_isr for preemptive scheduling
    set_handler(32, timer_isr as u64, IDT_PRESENT | IDT_INTERRUPT_GATE | IDT_DPL_KERNEL);

    // Vectors 33-47 = IRQ 1-15 (use generic ISR stubs)
    for i in 33..48 {
        let addr = isr_stub_table[i - 1]; // ISR stubs 32-47 map to table indices 32-47
        set_handler(i, addr, IDT_PRESENT | IDT_INTERRUPT_GATE | IDT_DPL_KERNEL);
    }

    // System call gate (int 0x80)
    set_handler(0x80, isr_stub_table[48], IDT_PRESENT | IDT_TRAP_GATE | IDT_DPL_USER);

    // APIC Spurious interrupt vector (0xFF)
    set_handler(0xFF, spurious_isr as u64, IDT_PRESENT | IDT_INTERRUPT_GATE | IDT_DPL_KERNEL);

    let descriptor = IdtDescriptor {
        limit: (IDT.len() * mem::size_of::<IdtEntry>() - 1) as u16,
        base: IDT.as_ptr() as u64,
    };

    core::arch::asm!(
        "lidt [{}]",
        in(reg) &descriptor,
        options(nostack)
    );
}

unsafe fn set_handler(vector: usize, handler_addr: u64, attributes: u8) {
    if vector >= IDT_ENTRIES {
        return;
    }

    IDT[vector] = IdtEntry {
        offset_low: (handler_addr & 0xFFFF) as u16,
        selector: 0x08,
        ist: 0,
        attributes,
        offset_middle: ((handler_addr >> 16) & 0xFFFF) as u16,
        offset_high: ((handler_addr >> 32) & 0xFFFFFFFF) as u32,
        reserved: 0,
    };
}

pub unsafe fn set_isr_handler(vector: usize, handler_addr: u64) {
    set_handler(vector, handler_addr, IDT_PRESENT | IDT_INTERRUPT_GATE | IDT_DPL_KERNEL);
}

pub unsafe fn set_user_handler(vector: usize, handler_addr: u64) {
    set_handler(vector, handler_addr, IDT_PRESENT | IDT_TRAP_GATE | IDT_DPL_USER);
}
