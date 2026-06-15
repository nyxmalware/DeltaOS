//! Local APIC + IO APIC Driver
//!
//! Manages the Advanced Programmable Interrupt Controller for
//! per-CPU timer interrupts, EOI handling, and interrupt routing.
//! Replaces the legacy 8259A PIC for modern x86_64 systems.

use crate::{kprintln, kprint, inl, outl, outb, PhysAddr};
use crate::acpi::AcpiInfo;

// ── Local APIC Register Offsets ─────────────────────────────────────

const LAPIC_ID: usize            = 0x020;
const LAPIC_VERSION: usize       = 0x030;
const LAPIC_TPR: usize           = 0x080;
const LAPIC_EOI: usize           = 0x0B0;
const LAPIC_SVR: usize           = 0x0F0;
const LAPIC_ISR_BASE: usize      = 0x100;
const LAPIC_TMR_BASE: usize      = 0x180;
const LAPIC_IRR_BASE: usize      = 0x200;
const LAPIC_ESR: usize           = 0x280;
const LAPIC_ICR_LOW: usize       = 0x300;
const LAPIC_ICR_HIGH: usize      = 0x310;
const LAPIC_LVT_TIMER: usize     = 0x320;
const LAPIC_LVT_THERMAL: usize   = 0x330;
const LAPIC_LVT_PERF: usize      = 0x340;
const LAPIC_LVT_LINT0: usize     = 0x350;
const LAPIC_LVT_LINT1: usize     = 0x360;
const LAPIC_LVT_ERROR: usize     = 0x370;
const LAPIC_TIMER_ICR: usize     = 0x380;  // Initial Count Register
const LAPIC_TIMER_CCR: usize     = 0x390;  // Current Count Register
const LAPIC_TIMER_DCR: usize     = 0x3E0;  // Divide Configuration Register

// ── Local APIC Constants ────────────────────────────────────────────

const LAPIC_SVR_ENABLE: u32          = 0x100;
const LAPIC_SVR_FOCUS_CHECK: u32     = 0x200;

/// Spurious interrupt vector (must be aligned to 16, but we use 0xFF)
pub const LAPIC_SPURIOUS_VECTOR: u32 = 0xFF;

/// Timer vector for scheduler preemption
pub const LAPIC_TIMER_VECTOR: u32 = 0x20; // Same as PIT IRQ0 vector

// ── LVT Timer Modes ─────────────────────────────────────────────────

const LAPIC_TIMER_MODE_ONESHOT: u32   = 0x00000;
const LAPIC_TIMER_MODE_PERIODIC: u32  = 0x20000;
const LAPIC_TIMER_MODE_TSC: u32       = 0x40000;

// ── LVT Mask bit ────────────────────────────────────────────────────

const LAPIC_LVT_MASKED: u32 = 0x10000;

// ── Timer Divide Values ─────────────────────────────────────────────

const LAPIC_TIMER_DIVIDE_BY_1: u32    = 0x0B;
const LAPIC_TIMER_DIVIDE_BY_2: u32    = 0x00;
const LAPIC_TIMER_DIVIDE_BY_4: u32    = 0x01;
const LAPIC_TIMER_DIVIDE_BY_8: u32    = 0x02;
const LAPIC_TIMER_DIVIDE_BY_16: u32   = 0x03;
const LAPIC_TIMER_DIVIDE_BY_32: u32   = 0x08;
const LAPIC_TIMER_DIVIDE_BY_64: u32   = 0x09;
const LAPIC_TIMER_DIVIDE_BY_128: u32  = 0x0A;
const LAPIC_TIMER_DIVIDE_BY_256: u32  = 0x0B;

// ── ICR Delivery Modes ──────────────────────────────────────────────

const ICR_DELIVERY_FIXED: u32      = 0x000000;
const ICR_DELIVERY_LOWEST: u32     = 0x100000;
const ICR_DELIVERY_SMI: u32        = 0x200000;
const ICR_DELIVERY_NMI: u32        = 0x400000;
const ICR_DELIVERY_INIT: u32       = 0x500000;
const ICR_DELIVERY_STARTUP: u32    = 0x600000;

const ICR_DEST_PHYSICAL: u32       = 0x000000;
const ICR_DEST_LOGICAL: u32        = 0x800000;

const ICR_LEVEL_ASSERT: u32        = 0x4000;
const ICR_LEVEL_DEASSERT: u32      = 0x0000;

// ── IO APIC Register Offsets ────────────────────────────────────────

const IOAPIC_REG_ID: u32         = 0x00;
const IOAPIC_REG_VERSION: u32    = 0x01;
const IOAPIC_REG_ARB: u32        = 0x02;
const IOAPIC_REG_REDIRECT: u32   = 0x10;

// ── IO APIC Redirect Entry Flags ────────────────────────────────────

const IOAPIC_REDIRECT_MASKED: u64       = 1 << 16;
const IOAPIC_REDIRECT_LEVEL: u64        = 1 << 15;
const IOAPIC_REDIRECT_LOW_PRIORITY: u64 = 1 << 13;
const IOAPIC_REDIRECT_LOGICAL: u64      = 1 << 11;
const IOAPIC_REDIRECT_EDGE: u64         = 0;

// ── Global State ────────────────────────────────────────────────────

static mut LAPIC_BASE: PhysAddr = 0;
static mut IOAPIC_BASE: PhysAddr = 0;
static mut APIC_INITIALIZED: bool = false;
static mut USING_APIC: bool = false;

// ── Local APIC Driver ───────────────────────────────────────────────

pub struct LocalApic;

impl LocalApic {
    /// Initialize the Local APIC using ACPI info
    pub fn init(acpi_info: &AcpiInfo) -> Result<(), &'static str> {
        unsafe {
            LAPIC_BASE = acpi_info.local_apic_address;
        }

        kprintln!("[APIC] Initializing Local APIC at {:#X}...", unsafe { LAPIC_BASE });

        // Mask all LVT entries first
        Self::write_reg(LAPIC_LVT_TIMER, LAPIC_LVT_MASKED);
        Self::write_reg(LAPIC_LVT_THERMAL, LAPIC_LVT_MASKED);
        Self::write_reg(LAPIC_LVT_PERF, LAPIC_LVT_MASKED);
        Self::write_reg(LAPIC_LVT_LINT0, LAPIC_LVT_MASKED);
        Self::write_reg(LAPIC_LVT_LINT1, LAPIC_LVT_MASKED);
        Self::write_reg(LAPIC_LVT_ERROR, LAPIC_LVT_MASKED);

        // Clear error status register (write twice to clear)
        Self::write_reg(LAPIC_ESR, 0);
        Self::write_reg(LAPIC_ESR, 0);

        // Set up Spurious Interrupt Vector
        // This enables the APIC by setting bit 8
        Self::write_reg(LAPIC_SVR, LAPIC_SVR_ENABLE | LAPIC_SPURIOUS_VECTOR);

        // Verify APIC is enabled
        let svr = Self::read_reg(LAPIC_SVR);
        if (svr & LAPIC_SVR_ENABLE) == 0 {
            return Err("Failed to enable Local APIC");
        }

        let apic_id = Self::read_reg(LAPIC_ID) >> 24;
        let version = Self::read_reg(LAPIC_VERSION) & 0xFF;
        let max_lvt = (Self::read_reg(LAPIC_VERSION) >> 16) & 0xFF;

        kprintln!("[APIC] LAPIC ID: {}, version: 0x{:X}, max LVT: {}", apic_id, version, max_lvt);

        unsafe {
            APIC_INITIALIZED = true;
            USING_APIC = true;
        }

        Ok(())
    }

    /// Set up the LAPIC timer in periodic mode for scheduler preemption
    /// `frequency_hz` - desired timer frequency (e.g. 100 for 100Hz = 10ms tick)
    pub fn setup_timer(frequency_hz: u32) {
        if !Self::is_available() {
            return;
        }

        kprintln!("[APIC] Setting up LAPIC timer at {} Hz...", frequency_hz);

        // Set divide value to 16
        Self::write_reg(LAPIC_TIMER_DCR, LAPIC_TIMER_DIVIDE_BY_16);

        // Calibrate: set initial count to max, wait for PIT calibration, then compute
        let bus_freq = Self::calibrate_timer();

        // Compute initial count for desired frequency
        // LAPIC timer frequency = bus_freq / divide_value
        // initial_count = (bus_freq / divide_value) / frequency_hz
        let divide: u64 = 16;
        let initial_count = (bus_freq / divide) / frequency_hz as u64;

        kprintln!("[APIC] Bus frequency: {} Hz, initial count: {}", bus_freq, initial_count);

        // Set periodic mode with our vector
        Self::write_reg(LAPIC_LVT_TIMER,
            LAPIC_TIMER_MODE_PERIODIC | LAPIC_TIMER_VECTOR);

        // Set initial count (starts the timer)
        Self::write_reg(LAPIC_TIMER_ICR, initial_count as u32);

        kprintln!("[APIC] LAPIC timer active - {} Hz periodic", frequency_hz);
    }

    /// Calibrate LAPIC timer using PIT
    /// Returns the estimated bus frequency in Hz
    fn calibrate_timer() -> u64 {
        // Set up PIT for calibration
        // PIT channel 2, mode 0 (one-shot), 1193182 Hz base frequency
        // We'll wait 10ms (11932 ticks)

        const PIT_FREQUENCY: u64 = 1193182;
        const CALIBRATION_MS: u64 = 50;
        const CALIBRATION_TICKS: u16 = (PIT_FREQUENCY * CALIBRATION_MS / 1000) as u16;

        // Configure PIT channel 2
        unsafe {
            outb(0x61, (inb_legacy(0x61) & 0x0D) | 0x01); // Enable PIT channel 2
            outb(0x43, 0xB2); // Channel 2, mode 0 (interrupt on terminal count), lobyte/hibyte
            outb(0x42, (CALIBRATION_TICKS & 0xFF) as u8);
            outb(0x42, ((CALIBRATION_TICKS >> 8) & 0xFF) as u8);

            // Reset LAPIC timer counter
            Self::write_reg(LAPIC_LVT_TIMER, LAPIC_TIMER_MODE_ONESHOT | LAPIC_TIMER_VECTOR);
            Self::write_reg(LAPIC_TIMER_DCR, LAPIC_TIMER_DIVIDE_BY_16);

            // Set max count to measure how many LAPIC ticks pass during PIT period
            Self::write_reg(LAPIC_TIMER_ICR, 0xFFFFFFFF);

            // Start PIT countdown
            outb(0x61, (inb_legacy(0x61) & 0x0D) | 0x01);

            // Wait for PIT to finish
            while (inb_legacy(0x61) & 0x20) == 0 {}

            // Read LAPIC current count
            let current_count = Self::read_reg(LAPIC_TIMER_CCR);
            let elapsed = 0xFFFFFFFFu32 - current_count;

            // Compute bus frequency
            // elapsed ticks happened in CALIBRATION_MS milliseconds
            let bus_freq = (elapsed as u64) * 16 * 1000 / CALIBRATION_MS;

            // Mask timer during calibration
            Self::write_reg(LAPIC_LVT_TIMER, LAPIC_LVT_MASKED);

            bus_freq
        }
    }

    /// Send End Of Interrupt
    #[inline(always)]
    pub fn eoi() {
        if Self::is_available() {
            Self::write_reg(LAPIC_EOI, 0);
        }
    }

    /// Get the LAPIC ID of the current processor
    pub fn id() -> u32 {
        if Self::is_available() {
            Self::read_reg(LAPIC_ID) >> 24
        } else {
            0
        }
    }

    /// Send an INIT IPI to a specific APIC
    pub fn send_init(apic_id: u8) {
        Self::write_reg(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
        Self::write_reg(LAPIC_ICR_LOW, ICR_DELIVERY_INIT | ICR_LEVEL_ASSERT | ICR_DEST_PHYSICAL);
        Self::wait_icr();
    }

    /// Send a STARTUP IPI to a specific APIC
    pub fn send_startup(apic_id: u8, vector: u8) {
        Self::write_reg(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
        Self::write_reg(LAPIC_ICR_LOW, ICR_DELIVERY_STARTUP | ((vector as u32) & 0xFF) | ICR_DEST_PHYSICAL);
        Self::wait_icr();
    }

    /// Wait for ICR delivery (poll delivery status bit)
    fn wait_icr() {
        for _ in 0..1000 {
            if (Self::read_reg(LAPIC_ICR_LOW) & 0x1000) == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Stop the LAPIC timer
    pub fn stop_timer() {
        if Self::is_available() {
            Self::write_reg(LAPIC_LVT_TIMER, LAPIC_LVT_MASKED);
            Self::write_reg(LAPIC_TIMER_ICR, 0);
        }
    }

    #[inline(always)]
    fn read_reg(offset: usize) -> u32 {
        unsafe {
            let base = LAPIC_BASE;
            core::ptr::read_volatile((base + offset) as *const u32)
        }
    }

    #[inline(always)]
    fn write_reg(offset: usize, value: u32) {
        unsafe {
            let base = LAPIC_BASE;
            core::ptr::write_volatile((base + offset) as *mut u32, value);
        }
    }

    pub fn is_available() -> bool {
        unsafe { APIC_INITIALIZED }
    }

    pub fn is_using_apic() -> bool {
        unsafe { USING_APIC }
    }

    /// Disable legacy PIC by masking all interrupts
    pub fn disable_pic() {
        kprintln!("[APIC] Disabling legacy PIC...");
        unsafe {
            outb(0x21, 0xFF); // Mask all IRQs on master PIC
            outb(0xA1, 0xFF); // Mask all IRQs on slave PIC
        }
    }
}

// ── IO APIC Driver ──────────────────────────────────────────────────

pub struct IoApic;

impl IoApic {
    /// Initialize the IO APIC
    pub fn init(acpi_info: &AcpiInfo) -> Result<(), &'static str> {
        if acpi_info.io_apic_address == 0 {
            kprintln!("[APIC] No IO APIC found, using legacy PIC");
            return Ok(());
        }

        unsafe {
            IOAPIC_BASE = acpi_info.io_apic_address;
        }

        kprintln!("[APIC] Initializing IO APIC at {:#X}...", acpi_info.io_apic_address);

        let version = Self::read(IOAPIC_REG_VERSION);
        let max_entries = ((version >> 16) & 0xFF) + 1;
        kprintln!("[APIC] IO APIC version: {}, max redirection entries: {}", version & 0xFF, max_entries);

        // Mask all redirection entries initially
        for i in 0..max_entries {
            Self::write_redirect(i, IOAPIC_REDIRECT_MASKED);
        }

        // Set up IRQ routing with interrupt overrides from ACPI
        // Map ISA IRQs to their GSI counterparts
        // IRQ 0 (timer) -> GSI 2 (usually remapped)
        // IRQ 1 (keyboard) -> GSI 1

        // Keyboard: IRQ 1 -> vector 33 (0x21)
        Self::setup_irq(1, 0x21, false);

        // Timer: set up later when scheduler starts
        // For now, if IRQ 0 is overridden to GSI 2, handle that
        for i in 0..acpi_info.int_override_count {
            let override_entry = &acpi_info.int_overrides[i];
            kprintln!("[APIC] IRQ override: bus={} irq={} gsi={} flags={:#X}",
                override_entry.bus_source,
                override_entry.irq_source,
                override_entry.global_system_interrupt,
                override_entry.flags);

            if override_entry.irq_source == 0 {
                // Timer IRQ override - map to our timer vector
                Self::setup_irq(
                    override_entry.global_system_interrupt as u8,
                    LAPIC_TIMER_VECTOR as u8,
                    (override_entry.flags & 0x0A) != 0, // Active low or level triggered
                );
            }
        }

        kprintln!("[APIC] IO APIC initialized");
        Ok(())
    }

    /// Set up a specific IRQ redirection entry
    /// `irq` - GSI number (IO APIC input pin)
    /// `vector` - IDT vector number
    /// `level_triggered` - true for level-triggered, false for edge-triggered
    pub fn setup_irq(irq: u8, vector: u8, level_triggered: bool) {
        let mut entry: u64 = vector as u64;
        if level_triggered {
            entry |= IOAPIC_REDIRECT_LEVEL;
        }
        // Destination: LAPIC ID 0 (BSP)
        // Delivery mode: Fixed (000)
        // Unmasked

        Self::write_redirect(irq, entry);
    }

    /// Mask a specific IRQ
    pub fn mask_irq(irq: u8) {
        let entry = Self::read_redirect(irq);
        Self::write_redirect(irq, entry | IOAPIC_REDIRECT_MASKED);
    }

    /// Unmask a specific IRQ
    pub fn unmask_irq(irq: u8) {
        let entry = Self::read_redirect(irq);
        Self::write_redirect(irq, entry & !IOAPIC_REDIRECT_MASKED);
    }

    fn read(register: u32) -> u32 {
        unsafe {
            let base = IOAPIC_BASE;
            core::ptr::write_volatile(base as *mut u32, register);
            core::ptr::read_volatile((base + 0x10) as *const u32)
        }
    }

    fn write(register: u32, value: u32) {
        unsafe {
            let base = IOAPIC_BASE;
            core::ptr::write_volatile(base as *mut u32, register);
            core::ptr::write_volatile((base + 0x10) as *mut u32, value);
        }
    }

    fn read_redirect(entry: u8) -> u64 {
        let reg = IOAPIC_REG_REDIRECT + (entry as u32) * 2;
        let low = Self::read(reg) as u64;
        let high = Self::read(reg + 1) as u64;
        (high << 32) | low
    }

    fn write_redirect(entry: u8, value: u64) {
        let reg = IOAPIC_REG_REDIRECT + (entry as u32) * 2;
        Self::write(reg, value as u32);
        Self::write(reg + 1, (value >> 32) as u32);
    }
}

// ── Legacy inb for PIT calibration ──────────────────────────────────

fn inb_legacy(port: u16) -> u8 {
    unsafe {
        let value: u8;
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack)
        );
        value
    }
}
