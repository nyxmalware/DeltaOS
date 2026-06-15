//! Task structure and process management
//!
//! Defines the Task struct with full register context for preemptive
//! multitasking, background/foreground state, and process lifecycle.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::VirtAddr;

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

pub const TASK_STACK_SIZE: usize = 64 * 1024;
pub const DEFAULT_TIME_SLICE: u64 = 10; // 10 ticks = 100ms at 100 Hz

// ── Task States ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,          // Waiting to be scheduled
    Running,        // Currently executing
    Blocked,        // Waiting for I/O, sleep, or signal
    Terminated,     // Has exited, awaiting cleanup
    Zombie,         // Exited but parent hasn't called wait()
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Ready => "READY",
            TaskState::Running => "RUNNING",
            TaskState::Blocked => "BLOCKED",
            TaskState::Terminated => "TERM",
            TaskState::Zombie => "ZOMBIE",
        }
    }
}

// ── Task Priority ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    RealTime = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Normal
    }
}

impl TaskPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskPriority::Low => "LOW",
            TaskPriority::Normal => "NORM",
            TaskPriority::High => "HIGH",
            TaskPriority::RealTime => "RT",
        }
    }
}

// ── Task Type ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    Kernel,
    User,
}

// ── Foreground/Background State ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskForeground {
    Foreground,    // Has terminal I/O control
    Background,    // Running without terminal
    Stopped,       // Stopped by signal (SIGTSTP/Ctrl+Z)
}

// ── Saved Registers for cooperative switching ───────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SavedRegisters {
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
}

impl Default for SavedRegisters {
    fn default() -> Self {
        SavedRegisters {
            rbx: 0,
            rbp: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: 0,
        }
    }
}

// ── Full Interrupt Context for preemptive switching ─────────────────
// This matches the register push order in timer_isr

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    // Pushed by our ISR stub (in reverse order)
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
    pub ds: u64,
    // Pushed by CPU
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// ── Task Structure ──────────────────────────────────────────────────

#[repr(C)]
pub struct Task {
    pub id: u64,
    pub name: String,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub task_type: TaskType,
    pub foreground: TaskForeground,

    // Stack management
    pub stack_top: VirtAddr,        // Top of the task's stack (highest address)
    pub kernel_stack: VirtAddr,     // Kernel stack for this task
    pub entry_point: VirtAddr,      // Task entry function

    // Register context
    pub regs: SavedRegisters,       // For cooperative switching
    pub saved_rsp: u64,             // Saved RSP for preemptive switching (points to InterruptFrame on stack)

    // Scheduling
    pub time_slice: u64,            // Remaining ticks in current quantum
    pub default_time_slice: u64,    // Default time quantum in ticks
    pub total_cpu_ticks: u64,       // Total CPU time consumed
    pub start_tick: u64,            // Tick when task was created

    // Process tree
    pub exit_code: i64,
    pub parent_id: Option<u64>,
    pub children: Vec<u64>,

    // Memory management
    pub pml4_addr: u64,             // Page table for this task (0 = kernel)

    // Process group / session
    pub pgid: u64,                  // Process group ID
    pub joining: bool,              // True if parent is waiting for this task

    // Signal handling
    pub pending_signals: u32,       // Bitmask of pending signals
    pub blocked_signals: u32,       // Bitmask of blocked signals
}

// ── Signal numbers ──────────────────────────────────────────────────

pub const SIGHUP: u32    = 1;
pub const SIGINT: u32    = 2;
pub const SIGKILL: u32   = 9;
pub const SIGTERM: u32   = 15;
pub const SIGTSTP: u32   = 20;  // Terminal stop (Ctrl+Z)
pub const SIGCONT: u32   = 18;  // Continue if stopped

impl Task {
    /// Create a new kernel task
    pub fn new_kernel(entry: VirtAddr, name: &str) -> Self {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);
        let stack_top = Self::alloc_stack(TASK_STACK_SIZE);

        let regs = SavedRegisters {
            rsp: stack_top as u64,
            rbp: stack_top as u64,
            ..SavedRegisters::default()
        };

        Task {
            id,
            name: String::from(name),
            state: TaskState::Ready,
            priority: TaskPriority::Normal,
            task_type: TaskType::Kernel,
            foreground: TaskForeground::Foreground,
            stack_top,
            kernel_stack: stack_top,
            entry_point: entry,
            regs,
            saved_rsp: stack_top as u64, // Will be set properly when first scheduled
            time_slice: DEFAULT_TIME_SLICE,
            default_time_slice: DEFAULT_TIME_SLICE,
            total_cpu_ticks: 0,
            start_tick: 0,
            exit_code: 0,
            parent_id: None,
            children: Vec::new(),
            pml4_addr: 0,
            pgid: id,
            joining: false,
            pending_signals: 0,
            blocked_signals: 0,
        }
    }

    /// Create a new user task
    pub fn new_user(entry: VirtAddr, name: &str) -> Self {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);
        let user_stack = Self::alloc_stack(TASK_STACK_SIZE);
        let kernel_stack = Self::alloc_stack(TASK_STACK_SIZE);

        let regs = SavedRegisters {
            rsp: user_stack as u64,
            rbp: user_stack as u64,
            ..SavedRegisters::default()
        };

        Task {
            id,
            name: String::from(name),
            state: TaskState::Ready,
            priority: TaskPriority::Normal,
            task_type: TaskType::User,
            foreground: TaskForeground::Foreground,
            stack_top: user_stack,
            kernel_stack,
            entry_point: entry,
            regs,
            saved_rsp: user_stack as u64,
            time_slice: DEFAULT_TIME_SLICE,
            default_time_slice: DEFAULT_TIME_SLICE,
            total_cpu_ticks: 0,
            start_tick: 0,
            exit_code: 0,
            parent_id: None,
            children: Vec::new(),
            pml4_addr: 0,
            pgid: id,
            joining: false,
            pending_signals: 0,
            blocked_signals: 0,
        }
    }

    /// Create a kernel task with a specific priority
    pub fn new_kernel_with_priority(entry: VirtAddr, name: &str, priority: TaskPriority) -> Self {
        let mut task = Self::new_kernel(entry, name);
        task.priority = priority;
        task
    }

    /// Create a background kernel task
    pub fn new_kernel_background(entry: VirtAddr, name: &str) -> Self {
        let mut task = Self::new_kernel(entry, name);
        task.foreground = TaskForeground::Background;
        task
    }

    fn alloc_stack(size: usize) -> VirtAddr {
        static mut NEXT_STACK: usize = 0xA00000;
        const STACK_REGION_END: usize = 0x2000000; // 32MB for stacks
        unsafe {
            if NEXT_STACK + size > STACK_REGION_END {
                return 0;
            }
            let addr = NEXT_STACK;
            NEXT_STACK += size;
            core::ptr::write_bytes(addr as *mut u8, 0, size);
            addr + size // Stack grows downward, so top = base + size
        }
    }

    /// Set up the initial stack for a task that will be resumed via iretq.
    /// This creates a fake interrupt frame on the stack so that when
    /// the scheduler first switches to this task, it can iretq into it.
    pub fn setup_initial_stack(&mut self) {
        // We need to construct an InterruptFrame on the stack that
        // will be "restored" when this task is first scheduled.
        // The timer_isr will pop these registers and iretq.

        unsafe {
            // Start from the top of the stack
            let mut rsp = self.stack_top as u64;

            // Push SS (kernel data segment)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0x10);

            // Push RSP (initial stack pointer for the task)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, self.stack_top as u64);

            // Push RFLAGS (interrupts enabled)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0x202); // IF flag set

            // Push CS (kernel code segment)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0x08);

            // Push RIP (entry point)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, self.entry_point as u64);

            // Push error code (0 for timer interrupt)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0);

            // Push vector (0x20 = timer, doesn't really matter for initial frame)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0x20);

            // Push DS (kernel data segment)
            rsp -= 8;
            core::ptr::write(rsp as *mut u64, 0x10);

            // Push general-purpose registers (initial values)
            // Order matches the pop order in timer_isr
            let zero_regs = [0u64; 15]; // rax, rbx, rcx, rdx, rsi, rdi, rbp, r8-r15
            for i in (0..15).rev() {
                rsp -= 8;
                core::ptr::write(rsp as *mut u64, zero_regs[i]);
            }

            // Set saved_rsp to point here - this is where the
            // timer_isr will start popping from
            self.saved_rsp = rsp;
            self.regs.rsp = rsp;
        }
    }

    pub fn priority_value(&self) -> u8 {
        self.priority as u8
    }

    pub fn set_time_slice(&mut self, slices: u64) {
        self.time_slice = slices;
        self.default_time_slice = slices;
    }

    pub fn reset_time_slice(&mut self) {
        self.time_slice = self.default_time_slice;
    }

    /// Send a signal to this task
    pub fn send_signal(&mut self, signal: u32) {
        if signal < 32 {
            self.pending_signals |= 1 << signal;
        }
    }

    /// Check if a specific signal is pending
    pub fn has_signal(&self, signal: u32) -> bool {
        if signal < 32 {
            (self.pending_signals & (1 << signal)) != 0
        } else {
            false
        }
    }

    /// Consume and return the next pending, non-blocked signal
    pub fn pop_signal(&mut self) -> Option<u32> {
        let pending = self.pending_signals & !self.blocked_signals;
        if pending == 0 {
            return None;
        }
        let signal = pending.trailing_zeros();
        self.pending_signals &= !(1 << signal);
        Some(signal)
    }

    /// Check if task is in foreground
    pub fn is_foreground(&self) -> bool {
        self.foreground == TaskForeground::Foreground
    }

    /// Check if task is stopped (Ctrl+Z)
    pub fn is_stopped(&self) -> bool {
        self.foreground == TaskForeground::Stopped
    }
}
