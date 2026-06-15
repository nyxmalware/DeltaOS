#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod memory;
pub mod sched;
pub mod fs;
pub mod module;
pub mod syscall;
pub mod interrupts;
pub mod arch;
pub mod drivers;
pub mod acpi;
pub mod apic;
pub mod timer;

pub use memory::pmm::Pmm;
pub use memory::vmm::Vmm;
pub use memory::heap::KernelHeap;
pub use sched::scheduler::Scheduler;
pub use sched::task::Task;
pub use fs::vfs::Vfs;
pub use module::manager::ModuleManager;
pub use acpi::AcpiParser;
pub use apic::{LocalApic, IoApic};

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SIZE_2MB: usize = 2 * 1024 * 1024;
pub const PAGE_SIZE_1GB: usize = 1024 * 1024 * 1024;
pub const KERNEL_BASE: usize = 0x100000;
pub const HEAP_BASE: usize = 0x800000;
pub const HEAP_SIZE: usize = 0x200000;
pub const PMM_BITMAP_BASE: usize = 0x500000;
pub const PMM_BITMAP_SIZE: usize = 0x200000;
pub const VMM_PAGES_BASE: usize = 0x700000;
pub const VMM_PAGES_SIZE: usize = 0x100000;
pub const TASK_STACKS_BASE: usize = 0xA00000;
pub const USER_SPACE_START: usize = 0x1_0000_0000;
pub const VGA_BUFFER: usize = 0x000B8000;
pub const PAGE_ENTRIES: usize = 512;

pub type PhysAddr = usize;
pub type VirtAddr = usize;
pub type PageFrame = usize;
pub type KernelResult<T> = Result<T, KernelError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    OutOfMemory,
    InvalidAddress,
    PageNotMapped,
    PageAlreadyMapped,
    InvalidArgument,
    DeviceNotFound,
    FileSystemNotFound,
    FileNotFound,
    IoError,
    ModuleNotFound,
    AccessDenied,
    GeneralFault,
}

impl From<KernelError> for isize {
    fn from(e: KernelError) -> isize {
        match e {
            KernelError::OutOfMemory => -12,
            KernelError::InvalidAddress => -14,
            KernelError::PageNotMapped => -2,
            KernelError::PageAlreadyMapped => -17,
            KernelError::InvalidArgument => -22,
            KernelError::DeviceNotFound => -19,
            KernelError::FileSystemNotFound => -19,
            KernelError::FileNotFound => -2,
            KernelError::IoError => -5,
            KernelError::ModuleNotFound => -2,
            KernelError::AccessDenied => -13,
            KernelError::GeneralFault => -1,
        }
    }
}

// ── I/O Port Operations ────────────────────────────────────────────

#[inline(always)]
pub fn inb(port: u16) -> u8 {
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

#[inline(always)]
pub fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack)
        );
    }
}

#[inline(always)]
pub fn inl(port: u16) -> u32 {
    unsafe {
        let value: u32;
        core::arch::asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") port,
            options(nomem, nostack)
        );
        value
    }
}

#[inline(always)]
pub fn outl(port: u16, value: u32) {
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack)
        );
    }
}

#[inline(always)]
pub fn cli() {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) }
}

#[inline(always)]
pub fn invlpg(addr: usize) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) addr, options(nomem, nostack));
    }
}

// ── VGA Console ────────────────────────────────────────────────────

use core::panic::PanicInfo;

const VGA_BUF: *mut u16 = 0x000B8000 as *mut u16;
const VGA_W: usize = 80;
const VGA_H: usize = 25;

static mut KERNEL_STATE: KernelState = KernelState::new();
static mut DEV_MODE: bool = false;

// Global scheduler pointer for timer ISR access
pub(crate) static mut KERNEL_SCHEDULER: Option<Scheduler> = None;

struct KernelState {
    pmm: Option<Pmm>,
    vmm: Option<Vmm>,
    vfs: Option<Vfs>,
    module_manager: Option<ModuleManager>,
    vga_row: usize,
    vga_col: usize,
    vga_color: u8,
}

impl KernelState {
    const fn new() -> Self {
        KernelState {
            pmm: None,
            vmm: None,
            vfs: None,
            module_manager: None,
            vga_row: 0,
            vga_col: 0,
            vga_color: 0x0F,
        }
    }
}

pub fn vga_print(msg: &str) {
    unsafe {
        for byte in msg.bytes() {
            if byte == b'\n' {
                KERNEL_STATE.vga_row += 1;
                KERNEL_STATE.vga_col = 0;
            } else {
                let offset = KERNEL_STATE.vga_row * VGA_W + KERNEL_STATE.vga_col;
                core::ptr::write_volatile(VGA_BUF.add(offset), (byte as u16) | ((KERNEL_STATE.vga_color as u16) << 8));
                KERNEL_STATE.vga_col += 1;
                if KERNEL_STATE.vga_col >= VGA_W {
                    KERNEL_STATE.vga_col = 0;
                    KERNEL_STATE.vga_row += 1;
                }
            }
            if KERNEL_STATE.vga_row >= VGA_H {
                for row in 1..VGA_H {
                    for col in 0..VGA_W {
                        let src = (row * VGA_W + col) as isize;
                        let dst = ((row - 1) * VGA_W + col) as isize;
                        let val = core::ptr::read_volatile(VGA_BUF.offset(src));
                        core::ptr::write_volatile(VGA_BUF.offset(dst), val);
                    }
                }
                for col in 0..VGA_W {
                    let offset = ((VGA_H - 1) * VGA_W + col) as isize;
                    core::ptr::write_volatile(VGA_BUF.offset(offset), 0x0F20);
                }
                KERNEL_STATE.vga_row = VGA_H - 1;
                KERNEL_STATE.vga_col = 0;
            }
        }
    }
    serial_print(msg);
}

const COM1: u16 = 0x3F8;

fn serial_init() {
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x80);
    outb(COM1 + 0, 0x03);
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x03);
    outb(COM1 + 2, 0xC7);
    outb(COM1 + 4, 0x0B);
}

fn serial_print(msg: &str) {
    for &byte in msg.as_bytes() {
        while (inb(COM1 + 5) & 0x20) == 0 {}
        outb(COM1, byte);
    }
}

pub fn vga_print_char(byte: u8) {
    let s = [byte];
    vga_print(unsafe { core::str::from_utf8_unchecked(&s) });
}

pub fn vga_set_color(fg: u8, bg: u8) {
    unsafe {
        KERNEL_STATE.vga_color = (bg << 4) | fg;
    }
}

pub fn vga_clear() {
    unsafe {
        for i in 0..(VGA_W * VGA_H) {
            core::ptr::write_volatile(VGA_BUF.add(i), 0x0F20);
        }
        KERNEL_STATE.vga_row = 0;
        KERNEL_STATE.vga_col = 0;
    }
}

macro_rules! kprint {
    ($($arg:tt)*) => {{
        struct VgaWriter;
        impl core::fmt::Write for VgaWriter {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::vga_print(s);
                Ok(())
            }
        }
        let _ = core::fmt::Write::write_fmt(&mut VgaWriter, format_args!($($arg)*));
    }};
}

pub(crate) use kprint;

macro_rules! kprintln {
    () => (kprint!("\n"));
    ($($arg:tt)*) => {
        kprint!("{}\n", format_args!($($arg)*))
    };
}

#[allow(unused_imports)]
pub(crate) use kprintln;

// ── RTC Clock ──────────────────────────────────────────────────────

fn read_rtc() -> (u8, u8, u8, u8, u8, u8) {
    fn read_cmos(reg: u8) -> u8 {
        crate::outb(0x70, reg);
        crate::inb(0x71)
    }
    fn bcd_to_binary(val: u8) -> u8 {
        (val & 0x0F) + ((val >> 4) * 10)
    }

    let seconds = bcd_to_binary(read_cmos(0x00));
    let minutes = bcd_to_binary(read_cmos(0x02));
    let hours = bcd_to_binary(read_cmos(0x04));
    let day = bcd_to_binary(read_cmos(0x07));
    let month = bcd_to_binary(read_cmos(0x08));
    let year = bcd_to_binary(read_cmos(0x09));
    (seconds, minutes, hours, day, month, year)
}

// ── Shell Commands ─────────────────────────────────────────────────

fn cmd_info() {
    kprintln!("========================================");
    kprintln!("  DeltaOS v0.2.0");
    kprintln!("  64-bit Rust Operating System");
    kprintln!("========================================");
    kprintln!("  Architecture:   x86_64 (long mode)");
    kprintln!("  Kernel:         Rust #![no_std]");
    kprintln!("  Bootloader:     NASM (multiboot2)");
    kprintln!("  Memory Manager: PMM (bitmap) + VMM (4-level paging)");
    kprintln!("  Heap:           Slab allocator (10 classes)");
    kprintln!("  Scheduler:      Round-Robin + priorities + preemptive");
    kprintln!("  VFS:            Trait-oriented (NTFS + ramfs)");
    kprintln!("  Drivers:        AHCI, NVMe, PS/2 Keyboard (C)");
    kprintln!("  Shell:          Kernel-space CLI");
    kprintln!("  Timer:          PIT + HPET + LAPIC Timer");
    kprintln!("  Interrupts:     PIC/APIC with preemptive scheduling");
    unsafe {
        if let Some(ref pmm) = KERNEL_STATE.pmm {
            kprintln!("  PMM free:       {} pages ({} KB)", pmm.free_page_count(), pmm.free_memory() / 1024);
            kprintln!("  PMM total:      {} pages ({} MB)", pmm.total_pages(), pmm.total_pages() * 4096 / 1024 / 1024);
        }
        kprintln!("  Dev mode:       {}", if DEV_MODE { "ON" } else { "OFF" });
        if LocalApic::is_available() {
            kprintln!("  APIC:           Local APIC active (ID: {})", LocalApic::id());
        } else {
            kprintln!("  APIC:           Legacy PIC mode");
        }
        if let Some(ref scheduler) = KERNEL_SCHEDULER {
            let stats = scheduler.stats();
            kprintln!("  Preemption:     {}", if stats.preemption_enabled { "ON" } else { "OFF" });
            kprintln!("  Tasks:          {} total, {} ready", stats.total_tasks, stats.ready_tasks);
            kprintln!("  Context sw:     {}", stats.context_switches);
        }
        kprintln!("  Uptime:         {} sec", timer::uptime_sec());
    }
    kprintln!("========================================");
}

fn cmd_time() {
    let (sec, min, hour, day, month, year) = read_rtc();
    kprintln!("RTC: 20{:02}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec);
    kprintln!("Uptime: {} ms ({} ticks)", timer::uptime_ms(), timer::tick_count());
}

fn cmd_dev() {
    unsafe {
        DEV_MODE = !DEV_MODE;
        if DEV_MODE {
            vga_set_color(0x0A, 0x00);
            kprintln!("[DEV] Developer mode ON");
            kprintln!("[DEV] Access: root");
            kprintln!("[DEV] Features: memory dump, register view, direct I/O");

            if let Some(ref pmm) = KERNEL_STATE.pmm {
                kprintln!("[DEV] PMM: {} free / {} total pages", pmm.free_page_count(), pmm.total_pages());
                kprintln!("[DEV] PMM bitmap at: {:#X}", PMM_BITMAP_BASE);
            }
            if let Some(ref _vmm) = KERNEL_STATE.vmm {
                kprintln!("[DEV] VMM: PML4 at CR3");
            }
            kprintln!("[DEV] Heap: {:#X}-{:#X}", HEAP_BASE, HEAP_BASE + HEAP_SIZE);
            let kend = get_kernel_end();
            kprintln!("[DEV] Kernel end: {:#X}", kend);
        } else {
            vga_set_color(0x0F, 0x00);
            kprintln!("[DEV] Developer mode OFF");
        }
    }
}

fn cmd_dev_mem(args: &[&str]) {
    if args.len() < 2 {
        kprintln!("Usage: mem <address>");
        return;
    }
    let addr = parse_hex(args[1]);
    if addr == 0 {
        kprintln!("Invalid address");
        return;
    }
    unsafe {
        let ptr = addr as *const u8;
        kprintln!("Memory at {:#X}:", addr);
        for i in 0..64 {
            if i % 16 == 0 {
                kprint!("\n  {:#X}: ", addr + i);
            }
            let val = core::ptr::read_volatile(ptr.add(i));
            kprint!("{:02X} ", val);
        }
        kprintln!();
    }
}

fn cmd_dev_regs() {
    unsafe {
        let cr0: u64;
        let cr2: u64;
        let cr3: u64;
        let cr4: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack));
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        kprintln!("CR0: {:#X}", cr0);
        kprintln!("CR2: {:#X}", cr2);
        kprintln!("CR3: {:#X}", cr3);
        kprintln!("CR4: {:#X}", cr4);
    }
}

fn cmd_dev_io(args: &[&str]) {
    if args.len() < 3 {
        kprintln!("Usage: io <in|out> <port> [value]");
        return;
    }
    let port = parse_hex(args[2]);
    if port == 0 && args[2] != "0" && args[2] != "0x0" {
        kprintln!("Invalid port");
        return;
    }
    if args[1] == "in" {
        let val = crate::inb(port as u16);
        kprintln!("IN {:#X}: {:#X}", port, val);
    } else if args[1] == "out" {
        if args.len() < 4 {
            kprintln!("Usage: io out <port> <value>");
            return;
        }
        let val = parse_hex(args[3]);
        crate::outb(port as u16, val as u8);
        kprintln!("OUT {:#X} <- {:#X}", port, val);
    }
}

fn cmd_help() {
    kprintln!("DeltaOS Shell v0.2.0 - Available commands:");
    kprintln!("  info          - Show system information");
    kprintln!("  time          - Show RTC time and uptime");
    kprintln!("  dev           - Toggle developer mode");
    kprintln!("  clear         - Clear screen");
    kprintln!("  help          - Show this help");
    kprintln!("  reboot        - Reboot system");
    kprintln!("");
    kprintln!("  Process Management:");
    kprintln!("  ps            - List all processes");
    kprintln!("  kill <id>     - Kill a process (SIGKILL)");
    kprintln!("  run <name>    - Run a kernel task");
    kprintln!("  bg <id>       - Resume stopped process in background");
    kprintln!("  fg <id>       - Bring process to foreground");
    kprintln!("  sched         - Show scheduler statistics");
    if unsafe { DEV_MODE } {
        vga_set_color(0x0A, 0x00);
        kprintln!("");
        kprintln!("[DEV] Developer commands:");
        kprintln!("  mem <addr>    - Dump memory at address");
        kprintln!("  regs          - Show CPU registers (CR0-CR4)");
        kprintln!("  io <in|out> <port> [val] - I/O port access");
        kprintln!("  pmm           - PMM status");
        kprintln!("  apic          - APIC status");
        kprintln!("  dev           - Turn off developer mode");
        vga_set_color(0x0F, 0x00);
    }
}

fn cmd_clear() {
    vga_clear();
}

fn cmd_reboot() {
    crate::outb(0x64, 0xFE);
}

fn cmd_dev_pmm() {
    unsafe {
        if let Some(ref pmm) = KERNEL_STATE.pmm {
            kprintln!("PMM Status:");
            kprintln!("  Total pages:   {}", pmm.total_pages());
            kprintln!("  Free pages:    {}", pmm.free_page_count());
            kprintln!("  Used pages:    {}", pmm.used_pages());
            kprintln!("  Free memory:   {} KB", pmm.free_memory() / 1024);
            kprintln!("  Used memory:   {} KB", pmm.used_memory() / 1024);
        } else {
            kprintln!("PMM not initialized");
        }
    }
}

fn cmd_ps() {
    unsafe {
        if let Some(ref scheduler) = KERNEL_SCHEDULER {
            let tasks = scheduler.list_tasks();
            if tasks.is_empty() {
                kprintln!("No tasks");
                return;
            }
            kprintln!("  PID  NAME             STATE  PRI  TYPE  FG  CPU_T");
            kprintln!("  ---  ----             -----  ---  ----  --  -----");
            for info in &tasks {
                kprintln!("  {:>3}  {:<16} {:<6} {:<4} {:<4}  {:<3} {}",
                    info.id,
                    info.name,
                    info.state_str(),
                    info.priority_str(),
                    match info.task_type {
                        sched::task::TaskType::Kernel => "K",
                        sched::task::TaskType::User => "U",
                    },
                    info.fg_str(),
                    info.cpu_ticks,
                );
            }
            kprintln!("  Total: {} tasks", tasks.len());
        } else {
            kprintln!("Scheduler not initialized");
        }
    }
}

fn cmd_kill(args: &[&str]) {
    if args.is_empty() {
        kprintln!("Usage: kill <pid>");
        return;
    }
    let pid = parse_int(args[0]);
    if pid == 0 {
        kprintln!("Invalid PID");
        return;
    }
    unsafe {
        if let Some(ref mut scheduler) = KERNEL_SCHEDULER {
            match scheduler.terminate_task(pid as u64, -9) {
                Ok(()) => kprintln!("Killed process {}", pid),
                Err(KernelError::AccessDenied) => kprintln!("Cannot kill system process {}", pid),
                Err(KernelError::InvalidArgument) => kprintln!("Process {} not found", pid),
                Err(e) => kprintln!("Error: {:?}", e),
            }
        }
    }
}

fn cmd_run(args: &[&str]) {
    if args.is_empty() {
        kprintln!("Usage: run <name>");
        kprintln!("Available: demo, counter, idle_test");
        return;
    }
    let name = args[0];
    let entry: VirtAddr = match name {
        "demo" => demo_task as *const () as usize,
        "counter" => counter_task as *const () as usize,
        "idle_test" => idle_test_task as *const () as usize,
        _ => {
            kprintln!("Unknown task: {}", name);
            kprintln!("Available: demo, counter, idle_test");
            return;
        }
    };

    unsafe {
        if let Some(ref mut scheduler) = KERNEL_SCHEDULER {
            let id = scheduler.create_kernel_task(entry, name);
            kprintln!("Started task '{}' with PID {}", name, id);
        } else {
            kprintln!("Scheduler not initialized");
        }
    }
}

fn cmd_bg(args: &[&str]) {
    if args.is_empty() {
        kprintln!("Usage: bg <pid>");
        return;
    }
    let pid = parse_int(args[0]);
    unsafe {
        if let Some(ref mut scheduler) = KERNEL_SCHEDULER {
            match scheduler.background_task(pid as u64) {
                Ok(()) => kprintln!("Process {} resumed in background", pid),
                Err(KernelError::InvalidArgument) => kprintln!("Process {} not found or not stopped", pid),
                Err(e) => kprintln!("Error: {:?}", e),
            }
        }
    }
}

fn cmd_fg(args: &[&str]) {
    if args.is_empty() {
        kprintln!("Usage: fg <pid>");
        return;
    }
    let pid = parse_int(args[0]);
    unsafe {
        if let Some(ref mut scheduler) = KERNEL_SCHEDULER {
            match scheduler.foreground_task(pid as u64) {
                Ok(()) => kprintln!("Process {} brought to foreground", pid),
                Err(KernelError::InvalidArgument) => kprintln!("Process {} not found or already foreground", pid),
                Err(e) => kprintln!("Error: {:?}", e),
            }
        }
    }
}

fn cmd_sched() {
    unsafe {
        if let Some(ref scheduler) = KERNEL_SCHEDULER {
            let stats = scheduler.stats();
            kprintln!("Scheduler Statistics:");
            kprintln!("  Total tasks:       {}", stats.total_tasks);
            kprintln!("  Ready tasks:       {}", stats.ready_tasks);
            kprintln!("  Tick count:        {}", stats.tick_count);
            kprintln!("  Context switches:  {}", stats.context_switches);
            kprintln!("  Preemption:        {}", if stats.preemption_enabled { "ENABLED" } else { "DISABLED" });
            kprintln!("  Current task:      {}", stats.current_task);
            kprintln!("  Timer frequency:   {} Hz", timer::timer_hz());
            kprintln!("  Uptime:            {} sec ({} ticks)", timer::uptime_sec(), timer::tick_count());
        } else {
            kprintln!("Scheduler not initialized");
        }
    }
}

fn cmd_dev_apic() {
    if LocalApic::is_available() {
        kprintln!("APIC Status:");
        kprintln!("  Local APIC:   ACTIVE (ID: {})", LocalApic::id());
        kprintln!("  Using APIC:   {}", LocalApic::is_using_apic());
    } else {
        kprintln!("APIC Status:");
        kprintln!("  Local APIC:   NOT AVAILABLE");
        kprintln!("  Using:        Legacy PIC");
    }
}

// ── Kernel Task Functions ───────────────────────────────────────────

fn demo_task() -> ! {
    kprintln!("[demo] Hello from demo task!");
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 10000000 == 0 {
            kprintln!("[demo] tick {}", count / 10000000);
        }
        // Yield to let other tasks run
        unsafe { core::arch::asm!("pause") };
    }
}

fn counter_task() -> ! {
    kprintln!("[counter] Counter task started");
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 50000000 == 0 {
            kprintln!("[counter] count = {}", count);
        }
        unsafe { core::arch::asm!("pause") };
    }
}

fn idle_test_task() -> ! {
    kprintln!("[idle_test] Idle test running");
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

// ── Utility Functions ───────────────────────────────────────────────

fn parse_hex(s: &str) -> usize {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = s.strip_prefix("0X").unwrap_or(s);
    let mut result: usize = 0;
    for ch in s.chars() {
        result *= 16;
        match ch {
            '0'..='9' => result += (ch as usize) - ('0' as usize),
            'a'..='f' => result += (ch as usize) - ('a' as usize) + 10,
            'A'..='F' => result += (ch as usize) - ('A' as usize) + 10,
            _ => return 0,
        }
    }
    result
}

fn parse_int(s: &str) -> usize {
    if s.starts_with("0x") || s.starts_with("0X") {
        parse_hex(s)
    } else {
        let mut result: usize = 0;
        for ch in s.chars() {
            result *= 10;
            match ch {
                '0'..='9' => result += (ch as usize) - ('0' as usize),
                _ => return 0,
            }
        }
        result
    }
}

fn execute_command(input: &str) {
    let input = input.trim();
    if input.is_empty() {
        return;
    }

    let parts: alloc::vec::Vec<&str> = input.split(' ').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return;
    }

    let cmd = parts[0];
    let args = &parts[1..];

    match cmd {
        "info" => cmd_info(),
        "time" => cmd_time(),
        "dev" => cmd_dev(),
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "reboot" => cmd_reboot(),
        "ps" => cmd_ps(),
        "kill" => cmd_kill(args),
        "run" => cmd_run(args),
        "bg" => cmd_bg(args),
        "fg" => cmd_fg(args),
        "sched" => cmd_sched(),
        "mem" => {
            if unsafe { DEV_MODE } { cmd_dev_mem(args) } else { kprintln!("Unknown command: {}", cmd) }
        }
        "regs" => {
            if unsafe { DEV_MODE } { cmd_dev_regs() } else { kprintln!("Unknown command: {}", cmd) }
        }
        "io" => {
            if unsafe { DEV_MODE } { cmd_dev_io(args) } else { kprintln!("Unknown command: {}", cmd) }
        }
        "pmm" => {
            if unsafe { DEV_MODE } { cmd_dev_pmm() } else { kprintln!("Unknown command: {}", cmd) }
        }
        "apic" => {
            if unsafe { DEV_MODE } { cmd_dev_apic() } else { kprintln!("Unknown command: {}", cmd) }
        }
        _ => kprintln!("Unknown command: {}", cmd),
    }
}

fn shell() -> ! {
    kprintln!("");
    kprintln!("DeltaOS Shell v0.2.0");
    kprintln!("Type 'help' for available commands.");
    kprintln!("");

    let mut input_buf: [u8; 256] = [0; 256];
    let mut input_pos: usize;

    loop {
        if unsafe { DEV_MODE } {
            vga_set_color(0x0A, 0x00);
        }
        kprint!("deltaos> ");
        vga_set_color(0x0F, 0x00);

        input_pos = 0;
        input_buf[0] = 0;

        loop {
            unsafe { core::arch::asm!("sti; hlt") };

            if interrupts::keyboard_has_data() {
                let ch = interrupts::keyboard_read_char();

                if ch == b'\n' {
                    kprintln!("");
                    input_buf[input_pos] = 0;
                    let input_str = unsafe { core::str::from_utf8_unchecked(&input_buf[..input_pos]) };
                    execute_command(input_str);
                    break;
                } else if ch == 8 {
                    if input_pos > 0 {
                        input_pos -= 1;
                        input_buf[input_pos] = 0;
                        vga_print_char(8);
                        vga_print_char(b' ');
                        vga_print_char(8);
                    }
                } else if ch >= 32 && input_pos < 255 {
                    input_buf[input_pos] = ch;
                    input_pos += 1;
                    vga_print_char(ch);
                }
            }
        }
    }
}

fn get_kernel_end() -> usize {
    extern "C" {
        static _kernel_end: u8;
    }
    unsafe { (&_kernel_end) as *const u8 as usize }
}

fn init_pic() {
    crate::outb(0x20, 0x11);
    crate::outb(0xA0, 0x11);
    crate::outb(0x21, 0x20);
    crate::outb(0xA1, 0x28);
    crate::outb(0x21, 0x04);
    crate::outb(0xA1, 0x02);
    crate::outb(0x21, 0x01);
    crate::outb(0xA1, 0x01);
    crate::outb(0x21, 0xFC);  // Enable IRQ 0 (timer) and IRQ 1 (keyboard) only
    crate::outb(0xA1, 0xFF);  // Mask all slave IRQs
}

// ── Kernel Entry Point ─────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn kernel_main(magic: u64, _mbi_info: u64) -> ! {
    serial_init();
    vga_clear();

    if magic != 0x36D76289 && magic != 0x2BADB002 {
        kprintln!("[PANIC] Invalid multiboot magic: {:#X}", magic);
        kprintln!("[PANIC] Expected: 0x36D76289 (multiboot2) or 0x2BADB002 (multiboot1)");
        loop { unsafe { core::arch::asm!("hlt") }; }
    }
    kprintln!("[BOOT] Multiboot magic validated: {:#X}", magic);

    // ── Phase 1: CPU Core Setup ──
    kprintln!("[INIT] Initializing GDT...");
    unsafe { arch::gdt::init(); }
    kprintln!("[INIT] GDT configured");

    kprintln!("[INIT] Remapping PIC...");
    init_pic();
    kprintln!("[INIT] PIC remapped (IRQ0=timer enabled)");

    kprintln!("[INIT] Setting up IDT...");
    unsafe { arch::idt::init(); }
    kprintln!("[INIT] IDT configured (timer ISR + spurious handler)");

    kprintln!("[INIT] Initializing interrupt handlers...");
    interrupts::init();
    kprintln!("[INIT] Interrupt handlers ready");

    kprintln!("[INIT] Enabling interrupts...");
    unsafe { core::arch::asm!("sti") };
    kprintln!("[INIT] Interrupts enabled");

    // ── Phase 2: Memory Management ──
    kprintln!("[INIT] Initializing Physical Memory Manager...");
    let mut pmm = unsafe { Pmm::new(KERNEL_BASE, 64 * 1024 * 1024 * 1024) };
    {
        let kend = get_kernel_end();
        if kend >= PMM_BITMAP_BASE {
            kprintln!("[WARN] Kernel end {:#X} overlaps PMM bitmap at {:#X}!", kend, PMM_BITMAP_BASE);
        }
        pmm.reserve_range(KERNEL_BASE, kend.saturating_sub(KERNEL_BASE).max(PMM_BITMAP_BASE - KERNEL_BASE));
        pmm.reserve_range(PMM_BITMAP_BASE, PMM_BITMAP_SIZE);
        pmm.reserve_range(VMM_PAGES_BASE, VMM_PAGES_SIZE);
        pmm.reserve_range(HEAP_BASE, HEAP_SIZE);
        pmm.reserve_range(TASK_STACKS_BASE, 0x400000);
    }
    unsafe { KERNEL_STATE.pmm = Some(pmm); }
    kprintln!("[INIT] PMM initialized");

    kprintln!("[INIT] Initializing Virtual Memory Manager...");
    let vmm = unsafe { Vmm::new() };
    unsafe { KERNEL_STATE.vmm = Some(vmm); }
    kprintln!("[INIT] VMM initialized");

    kprintln!("[INIT] Initializing kernel heap...");
    unsafe { KernelHeap::init(HEAP_BASE, HEAP_SIZE); }
    kprintln!("[INIT] Kernel heap initialized");

    // ── Phase 3: ACPI + APIC + Timer ──
    kprintln!("[INIT] Parsing ACPI tables...");
    let mut acpi_parser = AcpiParser::new();
    let acpi_info = match acpi_parser.parse() {
        Ok(info) => {
            kprintln!("[INIT] ACPI parsed successfully");
            info.clone()
        }
        Err(e) => {
            kprintln!("[WARN] ACPI parse failed: {}, using defaults", e);
            acpi_parser.info().clone()
        }
    };

    kprintln!("[INIT] Initializing timer subsystem...");
    timer::init(&acpi_info);
    kprintln!("[INIT] Timer subsystem active (PIT at {} Hz)", timer::timer_hz());

    // Try APIC initialization
    kprintln!("[INIT] Initializing Local APIC...");
    if LocalApic::init(&acpi_info).is_ok() {
        kprintln!("[INIT] Local APIC initialized");

        // Set up LAPIC timer for scheduler preemption
        LocalApic::setup_timer(100); // 100 Hz

        // Initialize IO APIC if available
        if acpi_info.io_apic_address != 0 {
            if IoApic::init(&acpi_info).is_ok() {
                kprintln!("[INIT] IO APIC initialized");
                // Disable legacy PIC since we have APIC
                LocalApic::disable_pic();
                kprintln!("[INIT] Switched to APIC mode (PIC disabled)");
            }
        }
    } else {
        kprintln!("[WARN] APIC not available, using legacy PIC");
    }

    // ── Phase 4: Filesystem and Modules ──
    kprintln!("[INIT] Initializing VFS...");
    let vfs = Vfs::new();
    unsafe { KERNEL_STATE.vfs = Some(vfs); }
    kprintln!("[INIT] VFS initialized");

    kprintln!("[INIT] Initializing module system...");
    let module_mgr = ModuleManager::new();
    unsafe { KERNEL_STATE.module_manager = Some(module_mgr); }
    kprintln!("[INIT] Module system ready");

    // ── Phase 5: Scheduler ──
    kprintln!("[INIT] Initializing scheduler...");
    let mut scheduler = Scheduler::new();

    // Create idle task (always available, lowest priority)
    scheduler.create_idle_task();
    kprintln!("[INIT] Idle task created (PID 1)");

    // Store scheduler in global state
    unsafe {
        KERNEL_SCHEDULER = Some(scheduler);
    }
    kprintln!("[INIT] Scheduler initialized");

    // ── Phase 6: Syscall Interface ──
    kprintln!("[INIT] Setting up system call interface...");
    syscall::init();
    kprintln!("[INIT] Syscall interface active");

    // ── Phase 7: Enable Preemption ──
    kprintln!("[INIT] Enabling preemptive scheduling...");
    unsafe {
        if let Some(ref mut scheduler) = KERNEL_SCHEDULER {
            scheduler.enable_preemption();
        }
    }

    // ── Boot Complete ──
    kprintln!("");
    kprintln!("========================================");
    kprintln!("  DeltaOS kernel initialized!");
    kprintln!("  Multitasking: ACTIVE (Round-Robin)");
    kprintln!("  All subsystems online.");
    kprintln!("========================================");
    kprintln!("");

    shell();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vga_set_color(0x0F, 0x04);
    kprintln!();
    kprintln!("!!! KERNEL PANIC !!!");
    kprintln!("{}", info);
    loop {
        unsafe { core::arch::asm!("cli; hlt") };
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Memory allocation error: size={}, align={}", layout.size(), layout.align());
}
