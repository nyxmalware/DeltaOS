//! Round-Robin Preemptive Scheduler
//!
//! Drives multitasking through timer interrupts. Supports:
//! - Preemptive context switching via LAPIC/PIT timer
//! - Priority-based Round-Robin scheduling
//! - Foreground/Background process management
//! - Process signals (SIGKILL, SIGTSTP, SIGCONT)
//! - Commands: ps, kill, run, bg, fg

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::task::{
    Task, TaskState, TaskPriority, TaskType, TaskForeground,
    SavedRegisters, DEFAULT_TIME_SLICE,
    SIGKILL, SIGTSTP, SIGCONT,
};
use crate::{VirtAddr, KernelResult, KernelError};
use crate::kprintln;
use crate::timer;

extern "C" {
    fn context_switch(current: *mut u8, next: *mut u8) -> ();
    fn preempt_switch(current_saved_ptr: *mut u64, next_rsp: u64) -> u64;
}

static mut DUMMY_REGS: SavedRegisters = SavedRegisters {
    rbx: 0,
    rbp: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
    rsp: 0,
};

// ── Scheduler ───────────────────────────────────────────────────────

pub struct Scheduler {
    ready_queue: VecDeque<Arc<Mutex<Task>>>,
    current: Option<Arc<Mutex<Task>>>,
    all_tasks: Vec<Arc<Mutex<Task>>>,
    task_count: u64,
    tick_count: u64,
    need_reschedule: bool,
    preempt_enabled: bool,
    scheduler_ticks: u64,    // Total scheduler ticks for uptime
    context_switches: u64,   // Total context switches performed
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            ready_queue: VecDeque::new(),
            current: None,
            all_tasks: Vec::new(),
            task_count: 0,
            tick_count: 0,
            need_reschedule: false,
            preempt_enabled: false,
            scheduler_ticks: 0,
            context_switches: 0,
        }
    }

    /// Create a kernel task and add it to the ready queue
    pub fn create_kernel_task(&mut self, entry: VirtAddr, name: &str) -> u64 {
        let mut task = Task::new_kernel(entry, name);
        task.setup_initial_stack();
        task.start_tick = timer::tick_count();
        let id = task.id;

        let arc_task = Arc::new(Mutex::new(task));
        self.ready_queue.push_back(Arc::clone(&arc_task));
        self.all_tasks.push(arc_task);
        self.task_count += 1;

        id
    }

    /// Create a kernel task with specific priority
    pub fn create_kernel_task_with_priority(
        &mut self,
        entry: VirtAddr,
        name: &str,
        priority: TaskPriority,
    ) -> u64 {
        let mut task = Task::new_kernel_with_priority(entry, name, priority);
        task.setup_initial_stack();
        task.start_tick = timer::tick_count();
        let id = task.id;

        let arc_task = Arc::new(Mutex::new(task));
        self.ready_queue.push_back(Arc::clone(&arc_task));
        self.all_tasks.push(arc_task);
        self.task_count += 1;

        id
    }

    /// Create a background kernel task
    pub fn create_background_task(&mut self, entry: VirtAddr, name: &str) -> u64 {
        let mut task = Task::new_kernel_background(entry, name);
        task.setup_initial_stack();
        task.start_tick = timer::tick_count();
        let id = task.id;

        let arc_task = Arc::new(Mutex::new(task));
        self.ready_queue.push_back(Arc::clone(&arc_task));
        self.all_tasks.push(arc_task);
        self.task_count += 1;

        id
    }

    /// Create a user task
    pub fn create_user_task(&mut self, entry: VirtAddr, name: &str) -> u64 {
        let mut task = Task::new_user(entry, name);
        task.setup_initial_stack();
        task.start_tick = timer::tick_count();
        let id = task.id;

        let arc_task = Arc::new(Mutex::new(task));
        self.ready_queue.push_back(Arc::clone(&arc_task));
        self.all_tasks.push(arc_task);
        self.task_count += 1;

        id
    }

    /// Create the idle task (lowest priority, always ready)
    pub fn create_idle_task(&mut self) -> u64 {
        let id = self.create_kernel_task_with_priority(
            idle_task as *const () as usize,
            "idle",
            TaskPriority::Low,
        );
        id
    }

    /// Called on every timer tick. Decrements the current task's time slice
    /// and triggers reschedule if the quantum has expired.
    pub fn tick(&mut self) {
        self.tick_count += 1;
        self.scheduler_ticks += 1;

        if !self.preempt_enabled {
            return;
        }

        if let Some(ref current_arc) = self.current {
            let mut current = current_arc.lock();

            // Check for pending signals
            if current.pending_signals != 0 {
                if let Some(signal) = current.pop_signal() {
                    match signal {
                        SIGKILL => {
                            current.state = TaskState::Terminated;
                            current.exit_code = -9;
                            drop(current);
                            self.need_reschedule = true;
                            return;
                        }
                        SIGTSTP => {
                            current.foreground = TaskForeground::Stopped;
                            current.state = TaskState::Blocked;
                            drop(current);
                            self.need_reschedule = true;
                            return;
                        }
                        SIGCONT => {
                            if current.foreground == TaskForeground::Stopped {
                                current.foreground = TaskForeground::Background;
                                current.state = TaskState::Running;
                            }
                        }
                        _ => {}
                    }
                }
            }

            if current.state != TaskState::Running {
                self.need_reschedule = true;
                return;
            }

            current.total_cpu_ticks += 1;

            if current.time_slice > 0 {
                current.time_slice -= 1;
            }

            if current.time_slice == 0 {
                self.need_reschedule = true;
            }
        }

        if self.need_reschedule {
            self.schedule();
        }
    }

    /// Perform the Round-Robin scheduling decision.
    /// Moves current task to back of queue, picks next task by priority.
    pub fn schedule(&mut self) {
        self.need_reschedule = false;

        // Move current task back to ready queue
        let current_arc = self.current.take();
        if let Some(ref arc) = current_arc {
            let mut task = arc.lock();
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
                task.reset_time_slice();
                drop(task);
                self.ready_queue.push_back(Arc::clone(arc));
            }
        }

        // Pick next task (priority-based Round-Robin)
        let next = self.pick_next_task();

        if let Some(next_arc) = next {
            {
                let mut next_task = next_arc.lock();
                next_task.state = TaskState::Running;
            }

            let should_switch = match &current_arc {
                Some(curr) => !Arc::ptr_eq(curr, &next_arc),
                None => true,
            };

            self.current = Some(Arc::clone(&next_arc));

            if should_switch {
                self.do_context_switch(&current_arc, &next_arc);
                self.context_switches += 1;
            }
        } else {
            // No ready tasks - keep current or idle
            self.current = current_arc;
        }
    }

    /// Pick the highest-priority ready task from the queue
    fn pick_next_task(&mut self) -> Option<Arc<Mutex<Task>>> {
        if self.ready_queue.is_empty() {
            return None;
        }

        // Find highest priority task
        let mut best_idx = 0;
        let mut best_priority = TaskPriority::Low;

        for (idx, arc) in self.ready_queue.iter().enumerate() {
            let task = arc.lock();
            if task.state == TaskState::Ready && task.priority > best_priority {
                best_priority = task.priority;
                best_idx = idx;
            }
        }

        if self.ready_queue[best_idx].lock().state != TaskState::Ready {
            return None;
        }

        self.ready_queue.remove(best_idx)
    }

    /// Perform the actual context switch (cooperative)
    fn do_context_switch(
        &mut self,
        current: &Option<Arc<Mutex<Task>>>,
        next: &Arc<Mutex<Task>>,
    ) {
        let current_regs_ptr = match current {
            Some(arc) => {
                let task = arc.lock();
                &task.regs as *const _ as *mut u8
            }
            None => unsafe { core::ptr::addr_of_mut!(DUMMY_REGS) as *mut SavedRegisters as *mut u8 },
        };

        let next_regs_ptr = {
            let task = next.lock();
            &task.regs as *const _ as *mut u8
        };

        unsafe {
            context_switch(current_regs_ptr, next_regs_ptr);
        }
    }

    // ── Preemptive switching ────────────────────────────────────────

    /// Called from the timer ISR with the current interrupt frame.
    /// Returns the RSP to use (either the same one for no-switch,
    /// or the new task's saved RSP for a context switch).
    pub fn preempt_schedule(&mut self, current_rsp: u64) -> u64 {
        if !self.preempt_enabled {
            return current_rsp;
        }

        // Save current task's RSP
        if let Some(ref current_arc) = self.current {
            let mut task = current_arc.lock();
            task.saved_rsp = current_rsp;
            task.state = TaskState::Ready;
            task.reset_time_slice();
            drop(task);
            self.ready_queue.push_back(Arc::clone(current_arc));
        }

        // Pick next task
        let next = self.pick_next_task();

        if let Some(next_arc) = next {
            {
                let mut next_task = next_arc.lock();
                next_task.state = TaskState::Running;
            }

            let next_rsp = {
                let task = next_arc.lock();
                task.saved_rsp
            };

            self.current = Some(next_arc);
            self.context_switches += 1;

            next_rsp
        } else {
            // No task to switch to - return current RSP
            // Restore current task
            if let Some(ref current_arc) = self.current {
                let mut task = current_arc.lock();
                task.state = TaskState::Running;
            }
            current_rsp
        }
    }

    // ── Process control operations ──────────────────────────────────

    /// Block the current task with a reason string
    pub fn block_current(&mut self, _reason: &str) {
        if let Some(ref arc) = self.current {
            let mut task = arc.lock();
            task.state = TaskState::Blocked;
            self.need_reschedule = true;
        }
    }

    /// Unblock a specific task by ID
    pub fn unblock_task(&mut self, task_id: u64) -> KernelResult<()> {
        for arc in &self.all_tasks {
            let mut task = arc.lock();
            if task.id == task_id && task.state == TaskState::Blocked {
                task.state = TaskState::Ready;
                drop(task);
                self.ready_queue.push_back(Arc::clone(arc));
                self.need_reschedule = true;
                return Ok(());
            }
        }
        Err(KernelError::InvalidArgument)
    }

    /// Terminate the current task
    pub fn terminate_current(&mut self, exit_code: i64) {
        if let Some(ref arc) = self.current {
            let mut task = arc.lock();
            task.state = TaskState::Terminated;
            task.exit_code = exit_code;
        }
        self.need_reschedule = true;
        self.schedule();
    }

    /// Terminate a specific task by ID (kill)
    pub fn terminate_task(&mut self, task_id: u64, exit_code: i64) -> KernelResult<()> {
        // Cannot kill the shell (task 1) or idle
        if task_id <= 2 {
            return Err(KernelError::AccessDenied);
        }

        for arc in &self.all_tasks {
            let mut task = arc.lock();
            if task.id == task_id {
                task.state = TaskState::Terminated;
                task.exit_code = exit_code;
                drop(task);
                self.need_reschedule = true;

                // Remove from ready queue if present
                self.ready_queue.retain(|a| a.lock().id != task_id);

                return Ok(());
            }
        }
        Err(KernelError::InvalidArgument)
    }

    /// Send a signal to a task
    pub fn send_signal(&mut self, task_id: u64, signal: u32) -> KernelResult<()> {
        for arc in &self.all_tasks {
            let mut task = arc.lock();
            if task.id == task_id {
                task.send_signal(signal);
                return Ok(());
            }
        }
        Err(KernelError::InvalidArgument)
    }

    /// Move a stopped task to background (bg command)
    pub fn background_task(&mut self, task_id: u64) -> KernelResult<()> {
        for arc in &self.all_tasks {
            let mut task = arc.lock();
            if task.id == task_id {
                if task.foreground != TaskForeground::Stopped {
                    return Err(KernelError::InvalidArgument);
                }
                task.foreground = TaskForeground::Background;
                task.state = TaskState::Ready;
                task.send_signal(SIGCONT);
                drop(task);
                self.ready_queue.push_back(Arc::clone(arc));
                self.need_reschedule = true;
                return Ok(());
            }
        }
        Err(KernelError::InvalidArgument)
    }

    /// Move a background/stopped task to foreground (fg command)
    pub fn foreground_task(&mut self, task_id: u64) -> KernelResult<()> {
        for arc in &self.all_tasks {
            let mut task = arc.lock();
            if task.id == task_id {
                if task.foreground == TaskForeground::Foreground {
                    return Err(KernelError::InvalidArgument); // Already foreground
                }
                task.foreground = TaskForeground::Foreground;
                if task.foreground == TaskForeground::Stopped {
                    task.state = TaskState::Ready;
                    task.send_signal(SIGCONT);
                    drop(task);
                    self.ready_queue.push_back(Arc::clone(arc));
                    self.need_reschedule = true;
                }
                return Ok(());
            }
        }
        Err(KernelError::InvalidArgument)
    }

    /// Yield the current task's remaining time slice
    pub fn yield_current(&mut self) {
        self.need_reschedule = true;
        self.schedule();
    }

    /// Enable preemptive scheduling
    pub fn enable_preemption(&mut self) {
        self.preempt_enabled = true;
        kprintln!("[SCHED] Preemptive scheduling ENABLED");
    }

    /// Disable preemptive scheduling
    pub fn disable_preemption(&mut self) {
        self.preempt_enabled = false;
        kprintln!("[SCHED] Preemptive scheduling DISABLED");
    }

    pub fn is_preemption_enabled(&self) -> bool {
        self.preempt_enabled
    }

    // ── Query operations ────────────────────────────────────────────

    pub fn current_task_id(&self) -> Option<u64> {
        self.current.as_ref().map(|arc| arc.lock().id)
    }

    pub fn current_task_name(&self) -> Option<String> {
        self.current.as_ref().map(|arc| arc.lock().name.clone())
    }

    pub fn task_count(&self) -> u64 {
        self.task_count
    }

    pub fn ready_count(&self) -> usize {
        self.ready_queue.len()
    }

    pub fn context_switches(&self) -> u64 {
        self.context_switches
    }

    pub fn get_task_info(&self, task_id: u64) -> Option<TaskInfo> {
        for arc in &self.all_tasks {
            let task = arc.lock();
            if task.id == task_id {
                return Some(TaskInfo {
                    id: task.id,
                    name: task.name.clone(),
                    state: task.state,
                    priority: task.priority,
                    task_type: task.task_type,
                    foreground: task.foreground,
                    entry_point: task.entry_point,
                    cpu_ticks: task.total_cpu_ticks,
                    exit_code: task.exit_code,
                });
            }
        }
        None
    }

    /// List all tasks (for ps command)
    pub fn list_tasks(&self) -> Vec<TaskInfo> {
        self.all_tasks
            .iter()
            .map(|arc| {
                let task = arc.lock();
                TaskInfo {
                    id: task.id,
                    name: task.name.clone(),
                    state: task.state,
                    priority: task.priority,
                    task_type: task.task_type,
                    foreground: task.foreground,
                    entry_point: task.entry_point,
                    cpu_ticks: task.total_cpu_ticks,
                    exit_code: task.exit_code,
                }
            })
            .collect()
    }

    /// Clean up terminated tasks (garbage collection)
    pub fn cleanup_terminated(&mut self) {
        let terminated: Vec<u64> = self.all_tasks
            .iter()
            .filter(|arc| arc.lock().state == TaskState::Terminated)
            .map(|arc| arc.lock().id)
            .collect();

        for id in terminated {
            self.all_tasks.retain(|arc| arc.lock().id != id);
            self.ready_queue.retain(|arc| arc.lock().id != id);
            // Don't decrement task_count since IDs are monotonic
        }
    }

    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            total_tasks: self.task_count,
            ready_tasks: self.ready_queue.len(),
            tick_count: self.tick_count,
            context_switches: self.context_switches,
            preemption_enabled: self.preempt_enabled,
            current_task: self.current_task_name().unwrap_or_else(|| String::from("none")),
        }
    }
}

// ── Task Info (for ps command) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub task_type: TaskType,
    pub foreground: TaskForeground,
    pub entry_point: VirtAddr,
    pub cpu_ticks: u64,
    pub exit_code: i64,
}

impl TaskInfo {
    pub fn state_str(&self) -> &'static str {
        self.state.as_str()
    }

    pub fn priority_str(&self) -> &'static str {
        self.priority.as_str()
    }

    pub fn fg_str(&self) -> &'static str {
        match self.foreground {
            TaskForeground::Foreground => "FG",
            TaskForeground::Background => "BG",
            TaskForeground::Stopped => "STP",
        }
    }
}

// ── Scheduler Statistics ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub total_tasks: u64,
    pub ready_tasks: usize,
    pub tick_count: u64,
    pub context_switches: u64,
    pub preemption_enabled: bool,
    pub current_task: String,
}

// ── Idle Task ───────────────────────────────────────────────────────

fn idle_task() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
