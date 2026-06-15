use crate::{PhysAddr, VirtAddr, PAGE_SIZE, PAGE_ENTRIES, KernelResult, KernelError};
use core::ptr;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct PageFlags: u64 {
        const PRESENT = 1 << 0;
        const WRITABLE = 1 << 1;
        const USER_ACCESSIBLE = 1 << 2;
        const WRITE_THROUGH = 1 << 3;
        const NO_CACHE = 1 << 4;
        const ACCESSED = 1 << 5;
        const DIRTY = 1 << 6;
        const HUGE_PAGE = 1 << 7;
        const GLOBAL = 1 << 8;
        const NO_EXECUTE = 1 << 63;
    }
}

impl Default for PageFlags {
    fn default() -> Self {
        PageFlags::PRESENT | PageFlags::WRITABLE
    }
}

type PageTableEntry = u64;

#[repr(C, align(4096))]
struct PageTable {
    entries: [PageTableEntry; PAGE_ENTRIES],
}

impl PageTable {
    #[allow(dead_code)]
    fn new_zeroed() -> Self {
        PageTable {
            entries: [0; PAGE_ENTRIES],
        }
    }

    #[inline]
    fn get_entry(&self, index: usize) -> PageTableEntry {
        unsafe { ptr::read_volatile(&self.entries[index]) }
    }

    #[inline]
    fn set_entry(&mut self, index: usize, value: PageTableEntry) {
        unsafe { ptr::write_volatile(&mut self.entries[index], value) }
    }

    #[inline]
    fn is_present(&self, index: usize) -> bool {
        (self.get_entry(index) & PageFlags::PRESENT.bits()) != 0
    }

    #[inline]
    fn get_next_table_addr(&self, index: usize) -> Option<PhysAddr> {
        let entry = self.get_entry(index);
        if entry & PageFlags::PRESENT.bits() == 0 {
            return None;
        }
        Some((entry & 0x000FFFFFFFFFF000) as PhysAddr)
    }
}

static mut NEXT_PAGE_ADDR: usize = 0x700000;
const VMM_PAGES_END: usize = 0x800000;

pub struct Vmm {
    pml4_addr: PhysAddr,
}

impl Vmm {
    pub unsafe fn new() -> Self {
        let pml4_addr = Self::read_cr3();
        Vmm { pml4_addr }
    }

    fn read_cr3() -> PhysAddr {
        let value: u64;
        unsafe {
            core::arch::asm!(
                "mov {}, cr3",
                out(reg) value,
                options(nomem, nostack)
            );
        }
        (value & 0x000FFFFFFFFFF000) as PhysAddr
    }

    pub fn write_cr3(&self, addr: PhysAddr) {
        unsafe {
            core::arch::asm!(
                "mov cr3, {}",
                in(reg) addr as u64,
                options(nomem, nostack)
            );
        }
    }

    pub fn map_page(
        &mut self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> KernelResult<()> {
        if virt % PAGE_SIZE != 0 || phys % PAGE_SIZE != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let pml4_index = (virt >> 39) & 0x1FF;
        let pdpt_index = (virt >> 30) & 0x1FF;
        let pd_index = (virt >> 21) & 0x1FF;
        let pt_index = (virt >> 12) & 0x1FF;

        unsafe {
            let pml4 = &mut *(self.pml4_addr as *mut PageTable);

            if !pml4.is_present(pml4_index) {
                let new_table = Self::alloc_page_table()?;
                let entry = (new_table as u64) | PageFlags::PRESENT.bits() | PageFlags::WRITABLE.bits();
                pml4.set_entry(pml4_index, entry);
            }

            let pdpt_addr = pml4.get_next_table_addr(pml4_index).unwrap();
            let pdpt = &mut *(pdpt_addr as *mut PageTable);

            if !pdpt.is_present(pdpt_index) {
                let new_table = Self::alloc_page_table()?;
                let entry = (new_table as u64) | PageFlags::PRESENT.bits() | PageFlags::WRITABLE.bits();
                pdpt.set_entry(pdpt_index, entry);
            }

            let pd_addr = pdpt.get_next_table_addr(pdpt_index).unwrap();
            let pd = &mut *(pd_addr as *mut PageTable);

            if !pd.is_present(pd_index) {
                let new_table = Self::alloc_page_table()?;
                let entry = (new_table as u64) | PageFlags::PRESENT.bits() | PageFlags::WRITABLE.bits();
                pd.set_entry(pd_index, entry);
            }

            let pt_addr = pd.get_next_table_addr(pd_index).unwrap();
            let pt = &mut *(pt_addr as *mut PageTable);

            if pt.is_present(pt_index) {
                return Err(KernelError::PageAlreadyMapped);
            }

            let entry = (phys as u64) | flags.bits();
            pt.set_entry(pt_index, entry);

            crate::invlpg(virt);
        }

        Ok(())
    }

    pub fn unmap_page(&mut self, virt: VirtAddr) -> KernelResult<PhysAddr> {
        if virt % PAGE_SIZE != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let pml4_index = (virt >> 39) & 0x1FF;
        let pdpt_index = (virt >> 30) & 0x1FF;
        let pd_index = (virt >> 21) & 0x1FF;
        let pt_index = (virt >> 12) & 0x1FF;

        unsafe {
            let pml4 = &mut *(self.pml4_addr as *mut PageTable);

            let pdpt_addr = pml4.get_next_table_addr(pml4_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pdpt = &mut *(pdpt_addr as *mut PageTable);

            let pd_addr = pdpt.get_next_table_addr(pdpt_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pd = &mut *(pd_addr as *mut PageTable);

            let pt_addr = pd.get_next_table_addr(pd_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pt = &mut *(pt_addr as *mut PageTable);

            if !pt.is_present(pt_index) {
                return Err(KernelError::PageNotMapped);
            }

            let old_entry = pt.get_entry(pt_index);
            let phys = (old_entry & 0x000FFFFFFFFFF000) as PhysAddr;

            pt.set_entry(pt_index, 0);

            crate::invlpg(virt);

            Ok(phys)
        }
    }

    pub fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        let pml4_index = (virt >> 39) & 0x1FF;
        let pdpt_index = (virt >> 30) & 0x1FF;
        let pd_index = (virt >> 21) & 0x1FF;
        let pt_index = (virt >> 12) & 0x1FF;
        let offset = virt & 0xFFF;

        unsafe {
            let pml4 = &*(self.pml4_addr as *const PageTable);

            let pdpt_addr = pml4.get_next_table_addr(pml4_index)?;
            let pdpt = &*(pdpt_addr as *const PageTable);

            if let Some(phys) = Self::check_huge_page(&pdpt, pdpt_index, virt, 30) {
                return Some(phys);
            }

            let pd_addr = pdpt.get_next_table_addr(pdpt_index)?;
            let pd = &*(pd_addr as *const PageTable);

            if let Some(phys) = Self::check_huge_page(&pd, pd_index, virt, 21) {
                return Some(phys);
            }

            let pt_addr = pd.get_next_table_addr(pd_index)?;
            let pt = &*(pt_addr as *const PageTable);

            if !pt.is_present(pt_index) {
                return None;
            }

            let entry = pt.get_entry(pt_index);
            let phys = (entry & 0x000FFFFFFFFFF000) as PhysAddr;
            Some(phys + offset)
        }
    }

    fn check_huge_page(table: &PageTable, index: usize, virt: VirtAddr, shift: usize) -> Option<PhysAddr> {
        let entry = table.get_entry(index);
        if entry & PageFlags::HUGE_PAGE.bits() != 0 && entry & PageFlags::PRESENT.bits() != 0 {
            let mask = (1 << shift) - 1;
            let phys = (entry & 0x000FFFFFFFFFF000) as PhysAddr;
            return Some(phys + (virt & mask));
        }
        None
    }

    pub fn identity_map(&mut self, start: PhysAddr, end: PhysAddr, flags: PageFlags) -> KernelResult<()> {
        let mut addr = start & !(PAGE_SIZE - 1);
        while addr < end {
            self.map_page(addr, addr, flags)?;
            addr += PAGE_SIZE;
        }
        Ok(())
    }

    fn alloc_page_table() -> KernelResult<PhysAddr> {
        unsafe {
            if NEXT_PAGE_ADDR + PAGE_SIZE > VMM_PAGES_END {
                return Err(KernelError::OutOfMemory);
            }
            let addr = NEXT_PAGE_ADDR;
            NEXT_PAGE_ADDR += PAGE_SIZE;
            ptr::write_bytes(addr as *mut u8, 0, PAGE_SIZE);
            Ok(addr)
        }
    }

    pub fn pml4_addr(&self) -> PhysAddr {
        self.pml4_addr
    }

    pub fn update_flags(&mut self, virt: VirtAddr, flags: PageFlags) -> KernelResult<()> {
        if virt % PAGE_SIZE != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let pml4_index = (virt >> 39) & 0x1FF;
        let pdpt_index = (virt >> 30) & 0x1FF;
        let pd_index = (virt >> 21) & 0x1FF;
        let pt_index = (virt >> 12) & 0x1FF;

        unsafe {
            let pml4 = &mut *(self.pml4_addr as *mut PageTable);

            let pdpt_addr = pml4.get_next_table_addr(pml4_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pdpt = &*(pdpt_addr as *const PageTable);

            let pd_addr = pdpt.get_next_table_addr(pdpt_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pd = &*(pd_addr as *const PageTable);

            let pt_addr = pd.get_next_table_addr(pd_index)
                .ok_or(KernelError::PageNotMapped)?;
            let pt = &mut *(pt_addr as *mut PageTable);

            if !pt.is_present(pt_index) {
                return Err(KernelError::PageNotMapped);
            }

            let old_entry = pt.get_entry(pt_index);
            let phys = old_entry & 0x000FFFFFFFFFF000;
            let new_entry = phys | flags.bits();
            pt.set_entry(pt_index, new_entry);

            crate::invlpg(virt);
        }

        Ok(())
    }
}
