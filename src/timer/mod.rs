//! Timer Subsystem
//!
//! Provides system timer services using:
//! - PIT (8254 Programmable Interval Timer) as fallback/scheduler timer
//! - HPET (High Precision Event Timer) for high-resolution timing
//! - LAPIC Timer for per-CPU scheduling (configured by apic module)
//!
//! The timer subsystem drives the scheduler's preemption mechanism.

use crate::{kprintln, outb, inb, PhysAddr};
use crate::acpi::AcpiInfo;

// ── PIT (8254) Constants ────────────────────────────────────────────

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_CHANNEL_1: u16 = 0x41;
const PIT_CHANNEL_2: u16 = 0x42;
const PIT_COMMAND: u16   = 0x43;

const PIT_BASE_FREQ: u64  = 1193182; // 1.193182 MHz

/// PIT operating modes
const PIT_MODE_INTERRUPT: u8  = 0; // Interrupt on terminal count
const PIT_MODE_ONESHOT: u8    = 1; // Hardware re-triggerable one-shot
const PIT_MODE_RATE_GEN: u8   = 2; // Rate generator
const PIT_MODE_SQUARE: u8     = 3; // Square wave generator

/// Access modes
const PIT_LATCH: u8    = 0;
const PIT_LO: u8       = 1;
const PIT_HI: u8       = 2;
const PIT_LOHI: u8     = 3;

// ── HPET Constants ──────────────────────────────────────────────────

const HPET_GENERAL_CAPS: usize      = 0x000;
const HPET_GENERAL_CONFIG: usize    = 0x010;
const HPET_GENERAL_INT_STATUS: usize = 0x020;
const HPET_MAIN_COUNTER: usize      = 0x0F0;
const HPET_TIMER0_CONFIG: usize     = 0x100;
const HPET_TIMER0_COMPARATOR: usize = 0x108;
const HPET_TIMER0_FSB_ROUTE: usize  = 0x110;

const HPET_CONFIG_ENABLE: u64        = 1 << 0;
const HPET_CONFIG_LEGACY_ROUTE: u64  = 1 << 1;

const HPET_TIMER_CONFIG_ENABLE: u64     = 1 << 0;
const HPET_TIMER_CONFIG_PERIODIC: u64   = 1 << 1;
const HPET_TIMER_CONFIG_IRQ_ENABLE: u64 = 1 << 2;
const HPET_TIMER_CONFIG_FSB_ENABLE: u64 = 1 << 5;
const HPET_TIMER_CONFIG_32BIT: u64      = 1 << 6;
const HPET_TIMER_CONFIG_FORCE_32: u64   = 1 << 7;

// ── Global State ────────────────────────────────────────────────────

static mut TICK_COUNT: u64 = 0;
static mut TIMER_HZ: u32 = 100;     // Default 100 Hz = 10ms tick
static mut HPET_BASE: PhysAddr = 0;
static mut HPET_PERIOD: u64 = 0;     // HPET counter period in femtoseconds
static mut HPET_AVAILABLE: bool = false;
static mut UPTIME_MS: u64 = 0;

// ── PIT Driver ──────────────────────────────────────────────────────

pub struct Pit;

impl Pit {
    /// Initialize PIT channel 0 in square wave mode at the given frequency.
    /// This is the primary timer for the scheduler when APIC is not available.
    pub fn init(frequency_hz: u32) {
        let divisor = if frequency_hz == 0 {
            0xFFFF // Slowest possible rate
        } else {
            let d = PIT_BASE_FREQ / frequency_hz as u64;
            if d > 65535 { 65535 } else if d < 1 { 1 } else { d as u16 }
        };

        unsafe {
            // Channel 0, lo/hi access, rate generator mode
            outb(PIT_COMMAND, (PIT_LOHI << 4) | (PIT_MODE_RATE_GEN << 1) | 0);

            // Set divisor
            outb(PIT_CHANNEL_0, (divisor & 0xFF) as u8);
            outb(PIT_CHANNEL_0, ((divisor >> 8) & 0xFF) as u8);
        }

        unsafe {
            TIMER_HZ = frequency_hz;
        }

        kprintln!("[TIMER] PIT initialized at {} Hz (divisor: {})", frequency_hz, divisor);
    }

    /// Read current PIT tick count (channel 0)
    pub fn current_count() -> u16 {
        unsafe {
            outb(PIT_COMMAND, 0); // Latch channel 0
            let lo = inb(PIT_CHANNEL_0);
            let hi = inb(PIT_CHANNEL_0);
            ((hi as u16) << 8) | (lo as u16)
        }
    }

    /// Simple delay using PIT channel 2
    pub fn delay_ms(ms: u32) {
        let ticks = (PIT_BASE_FREQ * ms as u64 / 1000) as u16;
        unsafe {
            outb(0x61, (inb(0x61) & 0x0D) | 0x01); // Enable channel 2
            outb(PIT_COMMAND, 0xB2); // Channel 2, mode 0, lo/hi
            outb(PIT_CHANNEL_2, (ticks & 0xFF) as u8);
            outb(PIT_CHANNEL_2, ((ticks >> 8) & 0xFF) as u8);
            outb(0x61, (inb(0x61) & 0x0D) | 0x01); // Start countdown
            while (inb(0x61) & 0x20) == 0 {} // Wait for terminal count
        }
    }

    /// Disable PIT timer (set to one-shot with maximum divisor, masked)
    pub fn disable() {
        unsafe {
            outb(PIT_COMMAND, (PIT_LOHI << 4) | (PIT_MODE_ONESHOT << 1) | 0);
            outb(PIT_CHANNEL_0, 0xFF);
            outb(PIT_CHANNEL_0, 0xFF);
        }
    }
}

// ── HPET Driver ─────────────────────────────────────────────────────

pub struct Hpet;

impl Hpet {
    /// Initialize HPET from ACPI info
    pub fn init(acpi_info: &AcpiInfo) -> Result<(), &'static str> {
        if acpi_info.hpet_address == 0 {
            kprintln!("[TIMER] HPET not available, using PIT");
            return Ok(());
        }

        unsafe {
            HPET_BASE = acpi_info.hpet_address;
        }

        kprintln!("[TIMER] Initializing HPET at {:#X}...", acpi_info.hpet_address);

        let caps = Self::read_reg(HPET_GENERAL_CAPS);
        let period = (caps >> 32) & 0xFFFFFFFF;
        let vendor_id = (caps >> 16) & 0xFFFF;
        let num_timers = ((caps >> 8) & 0x1F) + 1;
        let is_64bit = (caps & (1 << 13)) != 0;
        let legacy_route = (caps & (1 << 15)) != 0;

        if period == 0 || period > 100_000_000 {
            return Err("Invalid HPET period");
        }

        unsafe {
            HPET_PERIOD = period;
            HPET_AVAILABLE = true;
        }

        let freq_femto = 1_000_000_000_000_000u64 / period;
        let freq_mhz = freq_femto as f64 / 1_000_000.0;

        kprintln!("[TIMER] HPET: {} timers, {} bit, period: {} fs ({:.2} MHz)",
            num_timers, if is_64bit { "64" } else { "32" }, period, freq_mhz);
        kprintln!("[TIMER] HPET vendor: {:#X}, legacy route: {}", vendor_id, legacy_route);

        // Enable HPET with legacy route replacement
        let mut config = HPET_CONFIG_ENABLE;
        if legacy_route {
            config |= HPET_CONFIG_LEGACY_ROUTE;
        }
        Self::write_reg(HPET_GENERAL_CONFIG, config);

        // Clear main counter
        Self::write_reg(HPET_MAIN_COUNTER, 0);

        kprintln!("[TIMER] HPET initialized and enabled");

        Ok(())
    }

    /// Set up HPET Timer 0 as periodic timer for the scheduler
    pub fn setup_periodic(frequency_hz: u32) -> Result<(), &'static str> {
        if !Self::is_available() {
            return Err("HPET not available");
        }

        let period_fs = unsafe { HPET_PERIOD };
        let ticks_per_period = (1_000_000_000_000_000u64 / frequency_hz as u64) / period_fs;

        kprintln!("[TIMER] HPET periodic: {} Hz, {} ticks/period", frequency_hz, ticks_per_period);

        // Disable timer 0 first
        Self::write_reg(HPET_TIMER0_CONFIG, 0);

        // Set comparator value
        Self::write_reg(HPET_TIMER0_COMPARATOR, ticks_per_period);

        // Configure timer 0: periodic, enabled, IRQ enabled
        // Use IRQ0 (timer) routing in legacy mode
        let timer_config = HPET_TIMER_CONFIG_ENABLE
            | HPET_TIMER_CONFIG_PERIODIC
            | HPET_TIMER_CONFIG_IRQ_ENABLE;

        Self::write_reg(HPET_TIMER0_CONFIG, timer_config);

        kprintln!("[TIMER] HPET Timer 0 configured for {} Hz periodic", frequency_hz);

        Ok(())
    }

    /// Read the HPET main counter value
    pub fn counter() -> u64 {
        if Self::is_available() {
            Self::read_reg(HPET_MAIN_COUNTER)
        } else {
            0
        }
    }

    /// Get uptime in microseconds using HPET
    pub fn uptime_us() -> u64 {
        if !Self::is_available() {
            return unsafe { TICK_COUNT * 10000 / TIMER_HZ as u64 };
        }
        let count = Self::counter();
        let period_fs = unsafe { HPET_PERIOD };
        // Convert: count * period_fs / 1_000_000_000 = microseconds
        count * period_fs / 1_000_000_000
    }

    fn is_available() -> bool {
        unsafe { HPET_AVAILABLE }
    }

    fn read_reg(offset: usize) -> u64 {
        unsafe {
            let base = HPET_BASE;
            core::ptr::read_volatile((base + offset) as *const u64)
        }
    }

    fn write_reg(offset: usize, value: u64) {
        unsafe {
            let base = HPET_BASE;
            core::ptr::write_volatile((base + offset) as *mut u64, value);
        }
    }
}

// ── Timer Tick Interface ────────────────────────────────────────────

/// Called from the timer interrupt handler (IRQ 0 / LAPIC timer vector)
/// This is the heartbeat of the scheduler.
pub fn on_timer_tick() {
    unsafe {
        TICK_COUNT += 1;
        UPTIME_MS += 1000 / TIMER_HZ as u64;
    }
}

/// Get the total number of timer ticks since boot
pub fn tick_count() -> u64 {
    unsafe { TICK_COUNT }
}

/// Get the timer frequency in Hz
pub fn timer_hz() -> u32 {
    unsafe { TIMER_HZ }
}

/// Get uptime in milliseconds
pub fn uptime_ms() -> u64 {
    unsafe { UPTIME_MS }
}

/// Get uptime in seconds
pub fn uptime_sec() -> u64 {
    uptime_ms() / 1000
}

/// Initialize the timer subsystem
/// Sets up PIT as the primary scheduler timer, then tries HPET
pub fn init(acpi_info: &AcpiInfo) {
    let scheduler_hz = 100; // 100 Hz = 10ms preemption interval

    // Always initialize PIT as fallback
    Pit::init(scheduler_hz);

    // Try HPET
    if let Err(e) = Hpet::init(acpi_info) {
        kprintln!("[TIMER] HPET init failed: {}, using PIT", e);
    }
}

/// Switch to HPET for timer if available (after APIC init)
pub fn switch_to_hpet_if_available() {
    if Hpet::is_available() {
        kprintln!("[TIMER] Switching to HPET for scheduler timing...");
        // PIT stays as backup; HPET provides more precise timing
        // In legacy route mode, HPET Timer 0 replaces IRQ 0
    }
}
