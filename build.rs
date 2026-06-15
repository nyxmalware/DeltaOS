fn main() {
    println!("cargo:rerun-if-changed=boot/boot.asm");
    println!("cargo:rerun-if-changed=boot/long_mode.asm");
    println!("cargo:rerun-if-changed=src/sched/context_switch.asm");
    println!("cargo:rerun-if-changed=src/acpi/mod.rs");
    println!("cargo:rerun-if-changed=src/apic/mod.rs");
    println!("cargo:rerun-if-changed=src/timer/mod.rs");
    println!("cargo:rerun-if-changed=src/sched/task.rs");
    println!("cargo:rerun-if-changed=src/sched/scheduler.rs");
    println!("cargo:rerun-if-changed=src/interrupts.rs");
    println!("cargo:rerun-if-changed=src/arch/idt.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
