use core::mem;

const GDT_ENTRIES: usize = 7;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    flags_limit_high: u8,
    base_high: u8,
}

impl Default for GdtEntry {
    fn default() -> Self {
        GdtEntry {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            flags_limit_high: 0,
            base_high: 0,
        }
    }
}

#[repr(C, packed)]
struct GdtDescriptor {
    limit: u16,
    base: u64,
}

#[repr(C, align(16))]
#[derive(Default)]
pub struct Tss {
    pub reserved0: u32,
    pub rsp: [u64; 3],
    pub reserved1: u64,
    pub ist: [u64; 7],
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16,
}

static mut GDT: [GdtEntry; GDT_ENTRIES] = [GdtEntry {
    limit_low: 0,
    base_low: 0,
    base_middle: 0,
    access: 0,
    flags_limit_high: 0,
    base_high: 0,
}; GDT_ENTRIES];
static mut TSS: Tss = Tss {
    reserved0: 0,
    rsp: [0; 3],
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iomap_base: (mem::size_of::<Tss>() & 0xFFFF) as u16,
};

pub unsafe fn init() {
    GDT[0] = create_null_entry();
    GDT[1] = create_code_entry(0);
    GDT[2] = create_data_entry(0);
    GDT[3] = create_code_entry(3);
    GDT[4] = create_data_entry(3);

    let tss_base = core::ptr::addr_of!(TSS) as u64;
    let tss_limit = (mem::size_of::<Tss>() - 1) as u16;

    GDT[5] = create_tss_entry_low(tss_base, tss_limit);
    GDT[6] = create_tss_entry_high(tss_base);

    extern "C" {
        static _kernel_stack_top: u8;
    }
    TSS.rsp[0] = unsafe { (&_kernel_stack_top) as *const u8 as u64 };
    TSS.iomap_base = (mem::size_of::<Tss>() & 0xFFFF) as u16;

    let descriptor = GdtDescriptor {
        limit: (GDT.len() * mem::size_of::<GdtEntry>() - 1) as u16,
        base: GDT.as_ptr() as u64,
    };

    core::arch::asm!(
        "lgdt [{}]",
        in(reg) &descriptor,
        options(nostack)
    );

    reload_segments();

    core::arch::asm!(
        "ltr {0:x}",
        in(reg) 0x28u16,
        options(nostack)
    );
}

fn create_null_entry() -> GdtEntry {
    GdtEntry {
        limit_low: 0,
        base_low: 0,
        base_middle: 0,
        access: 0,
        flags_limit_high: 0,
        base_high: 0,
    }
}

fn create_code_entry(dpl: u8) -> GdtEntry {
    GdtEntry {
        limit_low: 0,
        base_low: 0,
        base_middle: 0,
        access: 0x9A | (dpl << 5),
        flags_limit_high: 0x20,
        base_high: 0,
    }
}

fn create_data_entry(dpl: u8) -> GdtEntry {
    GdtEntry {
        limit_low: 0,
        base_low: 0,
        base_middle: 0,
        access: 0x92 | (dpl << 5),
        flags_limit_high: 0,
        base_high: 0,
    }
}

fn create_tss_entry_low(base: u64, limit: u16) -> GdtEntry {
    GdtEntry {
        limit_low: limit,
        base_low: (base & 0xFFFF) as u16,
        base_middle: ((base >> 16) & 0xFF) as u8,
        access: 0x89,
        flags_limit_high: 0,
        base_high: ((base >> 24) & 0xFF) as u8,
    }
}

fn create_tss_entry_high(base: u64) -> GdtEntry {
    GdtEntry {
        limit_low: ((base >> 32) & 0xFFFF) as u16,
        base_low: ((base >> 48) & 0xFFFF) as u16,
        base_middle: 0,
        access: 0,
        flags_limit_high: 0,
        base_high: 0,
    }
}

unsafe fn reload_segments() {
    core::arch::asm!(
        "push 0x08",
        "lea rax, [rip + 2f]",
        "push rax",
        "retfq",
        "2:",
        options(nostack)
    );

    core::arch::asm!(
        "mov ax, 0x10",
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "mov gs, ax",
        "mov ss, ax",
        options(nostack)
    );
}

pub unsafe fn set_kernel_stack(stack: u64) {
    TSS.rsp[0] = stack;
}
