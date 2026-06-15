//! ACPI (Advanced Configuration and Power Interface) Table Parser
//!
//! Parses RSDP -> RSDT/XSDT -> MADT, HPET, FADT tables.
//! Provides LAPIC base address, IO APIC info, and HPET configuration
//! needed for APIC-based multitasking.

use crate::{kprintln, PhysAddr, VirtAddr};

// ── RSDP (Root System Description Pointer) ─────────────────────────

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,
    // ACPI 2.0+ fields
    pub length: u32,
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

impl Rsdp {
    /// Search for RSDP in BIOS memory regions:
    /// 1. EBDA (Extended BIOS Data Area) - first 1KB
    /// 2. BIOS ROM area 0xE0000 - 0xFFFFF
    pub fn find() -> Option<&'static Rsdp> {
        // Search EBDA
        let ebda_ptr = unsafe {
            let ptr = 0x40E as *const u16;
            (*ptr) as u32 as usize
        };
        if ebda_ptr != 0 {
            if let Some(rsdp) = Self::search_region(ebda_ptr, 1024) {
                return Some(rsdp);
            }
        }

        // Search BIOS ROM area
        if let Some(rsdp) = Self::search_region(0xE0000, 0x20000) {
            return Some(rsdp);
        }

        None
    }

    fn search_region(start: usize, size: usize) -> Option<&'static Rsdp> {
        let ptr = start as *const u8;
        for offset in (0..size).step_by(16) {
            unsafe {
                let p = ptr.add(offset);
                if core::ptr::read_volatile(p) != b'R' { continue; }
                if p.add(8) > ptr.add(size) { break; }

                let rsdp = &*(p as *const Rsdp);

                if &rsdp.signature == RSDP_SIGNATURE {
                    if Self::validate_checksum(rsdp) {
                        return Some(rsdp);
                    }
                }
            }
        }
        None
    }

    fn validate_checksum(rsdp: &Rsdp) -> bool {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                rsdp as *const Rsdp as *const u8,
                core::mem::size_of::<Rsdp>(),
            )
        };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        sum == 0
    }

    pub fn xsdt_address(&self) -> Option<PhysAddr> {
        if self.revision >= 2 && self.xsdt_address != 0 {
            Some(self.xsdt_address as PhysAddr)
        } else {
            None
        }
    }

    pub fn rsdt_address(&self) -> PhysAddr {
        self.rsdt_address as PhysAddr
    }
}

// ── SDT Header (common to all ACPI tables) ─────────────────────────

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub compiler_id: [u8; 4],
    pub compiler_revision: u32,
}

impl SdtHeader {
    pub fn signature_str(&self) -> &str {
        core::str::from_utf8(&self.signature).unwrap_or("????")
    }

    pub fn validate(&self) -> bool {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const SdtHeader as *const u8,
                self.length as usize,
            )
        };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        sum == 0
    }
}

// ── MADT (Multiple APIC Description Table) ──────────────────────────

const MADT_SIGNATURE: &[u8; 4] = b"APIC";

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Madt {
    pub header: SdtHeader,
    pub local_apic_address: u32,
    pub flags: u32,
    // Followed by variable-length interrupt controller structures
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum MadtEntryType {
    LocalApic = 0,
    IoApic = 1,
    InterruptSourceOverride = 2,
    NmiSource = 3,
    LocalApicNmi = 4,
    LocalApicAddressOverride = 5,
    IoSapic = 6,
    LocalSapic = 7,
    PlatformInterruptSource = 8,
    Unknown = 255,
}

impl From<u8> for MadtEntryType {
    fn from(v: u8) -> Self {
        match v {
            0 => MadtEntryType::LocalApic,
            1 => MadtEntryType::IoApic,
            2 => MadtEntryType::InterruptSourceOverride,
            3 => MadtEntryType::NmiSource,
            4 => MadtEntryType::LocalApicNmi,
            5 => MadtEntryType::LocalApicAddressOverride,
            6 => MadtEntryType::IoSapic,
            7 => MadtEntryType::LocalSapic,
            8 => MadtEntryType::PlatformInterruptSource,
            _ => MadtEntryType::Unknown,
        }
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtEntryHeader {
    pub entry_type: u8,
    pub length: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApicEntry {
    pub header: MadtEntryHeader,
    pub processor_id: u8,
    pub apic_id: u8,
    pub flags: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIoApicEntry {
    pub header: MadtEntryHeader,
    pub io_apic_id: u8,
    pub reserved: u8,
    pub io_apic_address: u32,
    pub global_system_interrupt_base: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIntSourceOverrideEntry {
    pub header: MadtEntryHeader,
    pub bus_source: u8,
    pub irq_source: u8,
    pub global_system_interrupt: u32,
    pub flags: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApicAddressOverrideEntry {
    pub header: MadtEntryHeader,
    pub reserved: u16,
    pub local_apic_address: u64,
}

// ── HPET (High Precision Event Timer) ───────────────────────────────

const HPET_SIGNATURE: &[u8; 4] = b"HPET";

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct HpetTable {
    pub header: SdtHeader,
    pub event_timer_block_id: u32,
    pub base_address: AcpiGenericAddress,
    pub hpet_number: u8,
    pub minimum_tick: u16,
    pub page_protection: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiGenericAddress {
    pub address_space_id: u8,
    pub register_bit_width: u8,
    pub register_bit_offset: u8,
    pub access_size: u8,
    pub address: u64,
}

impl AcpiGenericAddress {
    pub fn address(&self) -> u64 {
        self.address
    }
}

// ── FADT (Fixed ACPI Description Table) ─────────────────────────────

const FADT_SIGNATURE: &[u8; 4] = b"FACP";

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    pub header: SdtHeader,
    pub firmware_ctrl: u32,
    pub dsdt: u32,
    pub reserved0: u8,
    pub preferred_pm_profile: u8,
    pub sci_interrupt: u16,
    pub smi_command_port: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4bios_req: u8,
    pub pstate_cnt: u8,
    pub pm1a_event_block: u32,
    pub pm1b_event_block: u32,
    pub pm1a_control_block: u32,
    pub pm1b_control_block: u32,
    pub pm2_control_block: u32,
    pub pm_timer_block: u32,
    pub gpe0_block: u32,
    pub gpe1_block: u32,
    pub pm1_event_length: u8,
    pub pm1_control_length: u8,
    pub pm2_control_length: u8,
    pub pm_timer_length: u8,
    pub gpe0_block_length: u8,
    pub gpe1_block_length: u8,
    pub gpe1_base: u8,
    pub cstate_cnt: u8,
    pub p_lvl2_lat: u16,
    pub p_lvl3_lat: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alrm: u8,
    pub mon_alrm: u8,
    pub century: u8,
    pub iapc_boot_arch: u16,
    pub reserved1: u8,
    pub flags: u32,
    pub reset_reg: AcpiGenericAddress,
    pub reset_value: u8,
    pub reserved2: [u8; 3],
    pub x_firmware_ctrl: u64,
    pub x_dsdt: u64,
    pub x_pm1a_event_block: AcpiGenericAddress,
    pub x_pm1b_event_block: AcpiGenericAddress,
    pub x_pm1a_control_block: AcpiGenericAddress,
    pub x_pm1b_control_block: AcpiGenericAddress,
    pub x_pm2_control_block: AcpiGenericAddress,
    pub x_pm_timer_block: AcpiGenericAddress,
    pub x_gpe0_block: AcpiGenericAddress,
    pub x_gpe1_block: AcpiGenericAddress,
}

// ── Parsed ACPI Information ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AcpiInfo {
    pub local_apic_address: PhysAddr,
    pub io_apic_address: PhysAddr,
    pub io_apic_gsi_base: u32,
    pub hpet_address: PhysAddr,
    pub hpet_number: u8,
    pub hpet_min_tick: u16,
    pub processor_count: u8,
    pub processors: [ProcessorInfo; 256],
    pub int_overrides: [IntOverride; 16],
    pub int_override_count: usize,
    pub is_acpi_2: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessorInfo {
    pub processor_id: u8,
    pub apic_id: u8,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct IntOverride {
    pub bus_source: u8,
    pub irq_source: u8,
    pub global_system_interrupt: u32,
    pub flags: u16,
}

impl AcpiInfo {
    pub fn new() -> Self {
        AcpiInfo {
            local_apic_address: 0xFEE00000, // Default LAPIC address
            io_apic_address: 0xFEC00000,    // Default IO APIC address
            io_apic_gsi_base: 0,
            hpet_address: 0,
            hpet_number: 0,
            hpet_min_tick: 0,
            processor_count: 0,
            processors: [ProcessorInfo {
                processor_id: 0,
                apic_id: 0,
                is_enabled: false,
            }; 256],
            int_overrides: [IntOverride {
                bus_source: 0,
                irq_source: 0,
                global_system_interrupt: 0,
                flags: 0,
            }; 16],
            int_override_count: 0,
            is_acpi_2: false,
        }
    }
}

// ── ACPI Parser ─────────────────────────────────────────────────────

pub struct AcpiParser {
    info: AcpiInfo,
}

impl AcpiParser {
    pub fn new() -> Self {
        AcpiParser {
            info: AcpiInfo::new(),
        }
    }

    /// Full ACPI initialization: find RSDP, parse tables, extract info
    pub fn parse(&mut self) -> Result<&AcpiInfo, &'static str> {
        kprintln!("[ACPI] Searching for RSDP...");

        let rsdp = Rsdp::find().ok_or("RSDP not found")?;
        kprintln!("[ACPI] RSDP found at {:#X}, revision {}", rsdp as *const _ as usize, rsdp.revision);

        self.info.is_acpi_2 = rsdp.revision >= 2;

        // Use XSDT if available (ACPI 2.0+), otherwise RSDT
        if let Some(xsdt_addr) = rsdp.xsdt_address() {
            kprintln!("[ACPI] Using XSDT at {:#X}", xsdt_addr);
            self.parse_xsdt(xsdt_addr)?;
        } else {
            let rsdt_addr = rsdp.rsdt_address();
            kprintln!("[ACPI] Using RSDT at {:#X}", rsdt_addr);
            self.parse_rsdt(rsdt_addr)?;
        }

        kprintln!("[ACPI] Found {} processors", self.info.processor_count);
        kprintln!("[ACPI] LAPIC address: {:#X}", self.info.local_apic_address);
        if self.info.io_apic_address != 0 {
            kprintln!("[ACPI] IO APIC address: {:#X}", self.info.io_apic_address);
        }
        if self.info.hpet_address != 0 {
            kprintln!("[ACPI] HPET address: {:#X}", self.info.hpet_address);
        }

        Ok(&self.info)
    }

    fn parse_xsdt(&mut self, xsdt_addr: PhysAddr) -> Result<(), &'static str> {
        let header = unsafe { &*(xsdt_addr as *const SdtHeader) };

        if &header.signature != b"XSDT" {
            return Err("Invalid XSDT signature");
        }

        if !header.validate() {
            return Err("XSDT checksum invalid");
        }

        let entry_count = (header.length as usize - core::mem::size_of::<SdtHeader>()) / 8;
        let entries_ptr = unsafe {
            (xsdt_addr + core::mem::size_of::<SdtHeader>()) as *const u64
        };

        for i in 0..entry_count {
            let addr = unsafe { core::ptr::read_volatile(entries_ptr.add(i)) } as PhysAddr;
            if addr == 0 { continue; }
            let _ = self.parse_table(addr);
        }

        Ok(())
    }

    fn parse_rsdt(&mut self, rsdt_addr: PhysAddr) -> Result<(), &'static str> {
        let header = unsafe { &*(rsdt_addr as *const SdtHeader) };

        if &header.signature != b"RSDT" {
            return Err("Invalid RSDT signature");
        }

        if !header.validate() {
            return Err("RSDT checksum invalid");
        }

        let entry_count = (header.length as usize - core::mem::size_of::<SdtHeader>()) / 4;
        let entries_ptr = unsafe {
            (rsdt_addr + core::mem::size_of::<SdtHeader>()) as *const u32
        };

        for i in 0..entry_count {
            let addr = unsafe { core::ptr::read_volatile(entries_ptr.add(i)) } as PhysAddr;
            if addr == 0 { continue; }
            let _ = self.parse_table(addr);
        }

        Ok(())
    }

    fn parse_table(&mut self, addr: PhysAddr) -> Result<(), &'static str> {
        let header = unsafe { &*(addr as *const SdtHeader) };

        match &header.signature {
            MADT_SIGNATURE => {
                kprintln!("[ACPI] Found MADT at {:#X}", addr);
                self.parse_madt(addr)?;
            }
            HPET_SIGNATURE => {
                kprintln!("[ACPI] Found HPET at {:#X}", addr);
                self.parse_hpet(addr)?;
            }
            FADT_SIGNATURE => {
                kprintln!("[ACPI] Found FADT at {:#X}", addr);
                // FADT parsed for future power management
            }
            _ => {
                // Unknown table, skip
            }
        }

        Ok(())
    }

    fn parse_madt(&mut self, addr: PhysAddr) -> Result<(), &'static str> {
        let madt = unsafe { &*(addr as *const Madt) };

        // Default LAPIC address from MADT header
        self.info.local_apic_address = madt.local_apic_address as PhysAddr;

        // Parse variable-length entries
        let header_size = core::mem::size_of::<Madt>();
        let total_length = madt.header.length as usize;
        let mut offset = header_size;

        while offset < total_length {
            let entry_header = unsafe {
                &*((addr + offset) as *const MadtEntryHeader)
            };

            match MadtEntryType::from(entry_header.entry_type) {
                MadtEntryType::LocalApic => {
                    let entry = unsafe {
                        &*((addr + offset) as *const MadtLocalApicEntry)
                    };
                    let is_enabled = (entry.flags & 1) != 0;
                    if self.info.processor_count < 256 {
                        self.info.processors[self.info.processor_count] = ProcessorInfo {
                            processor_id: entry.processor_id,
                            apic_id: entry.apic_id,
                            is_enabled,
                        };
                        self.info.processor_count += 1;
                    }
                }
                MadtEntryType::IoApic => {
                    let entry = unsafe {
                        &*((addr + offset) as *const MadtIoApicEntry)
                    };
                    self.info.io_apic_address = entry.io_apic_address as PhysAddr;
                    self.info.io_apic_gsi_base = entry.global_system_interrupt_base;
                }
                MadtEntryType::InterruptSourceOverride => {
                    let entry = unsafe {
                        &*((addr + offset) as *const MadtIntSourceOverrideEntry)
                    };
                    if self.info.int_override_count < 16 {
                        self.info.int_overrides[self.info.int_override_count] = IntOverride {
                            bus_source: entry.bus_source,
                            irq_source: entry.irq_source,
                            global_system_interrupt: entry.global_system_interrupt,
                            flags: entry.flags,
                        };
                        self.info.int_override_count += 1;
                    }
                }
                MadtEntryType::LocalApicAddressOverride => {
                    let entry = unsafe {
                        &*((addr + offset) as *const MadtLocalApicAddressOverrideEntry)
                    };
                    self.info.local_apic_address = entry.local_apic_address as PhysAddr;
                }
                _ => {}
            }

            if entry_header.length == 0 {
                break; // Prevent infinite loop on malformed data
            }
            offset += entry_header.length as usize;
        }

        Ok(())
    }

    fn parse_hpet(&mut self, addr: PhysAddr) -> Result<(), &'static str> {
        let hpet = unsafe { &*(addr as *const HpetTable) };

        self.info.hpet_address = hpet.base_address.address() as PhysAddr;
        self.info.hpet_number = hpet.hpet_number;
        self.info.hpet_min_tick = hpet.minimum_tick;

        Ok(())
    }

    pub fn info(&self) -> &AcpiInfo {
        &self.info
    }
}
