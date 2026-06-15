use crate::{PhysAddr, PAGE_SIZE, KernelResult, KernelError, PMM_BITMAP_BASE};

const MAX_PHYS_MEMORY: usize = 64 * 1024 * 1024 * 1024;
const MAX_PAGES: usize = MAX_PHYS_MEMORY / PAGE_SIZE;
const BITMAP_SIZE: usize = MAX_PAGES / 8;
const BITMAP_BASE: usize = PMM_BITMAP_BASE;

pub struct Pmm {
    bitmap: &'static mut [u8],
    total_pages: usize,
    free_page_count: usize,
    base_addr: PhysAddr,
    end_addr: PhysAddr,
}

impl Pmm {
    pub unsafe fn new(base_addr: PhysAddr, memory_size: usize) -> Self {
        let total_pages = memory_size / PAGE_SIZE;
        let bitmap_ptr = BITMAP_BASE as *mut u8;

        core::ptr::write_bytes(bitmap_ptr, 0, BITMAP_SIZE);

        let bitmap = core::slice::from_raw_parts_mut(bitmap_ptr, BITMAP_SIZE);

        let mut pmm = Pmm {
            bitmap,
            total_pages,
            free_page_count: total_pages,
            base_addr,
            end_addr: base_addr + memory_size,
        };

        let bitmap_start_page = (BITMAP_BASE - base_addr) / PAGE_SIZE;
        let bitmap_pages = (BITMAP_SIZE + PAGE_SIZE - 1) / PAGE_SIZE;
        for i in bitmap_start_page..(bitmap_start_page + bitmap_pages) {
            if i < total_pages {
                pmm.set_bit(i);
                pmm.free_page_count = pmm.free_page_count.saturating_sub(1);
            }
        }

        pmm
    }

    pub fn alloc_page(&mut self) -> KernelResult<PhysAddr> {
        if self.free_page_count == 0 {
            return Err(KernelError::OutOfMemory);
        }

        for i in 0..self.total_pages {
            if !self.get_bit(i) {
                self.set_bit(i);
                self.free_page_count -= 1;
                let addr = self.base_addr + i * PAGE_SIZE;
                unsafe {
                    core::ptr::write_bytes(addr as *mut u8, 0, PAGE_SIZE);
                }
                return Ok(addr);
            }
        }

        Err(KernelError::OutOfMemory)
    }

    pub fn alloc_pages(&mut self, count: usize) -> KernelResult<PhysAddr> {
        if self.free_page_count < count {
            return Err(KernelError::OutOfMemory);
        }

        let mut found_start: Option<usize> = None;
        let mut consecutive = 0;

        for i in 0..self.total_pages {
            if !self.get_bit(i) {
                if found_start.is_none() {
                    found_start = Some(i);
                }
                consecutive += 1;
                if consecutive == count {
                    let start = found_start.unwrap();
                    for j in start..(start + count) {
                        self.set_bit(j);
                    }
                    self.free_page_count -= count;
                    let addr = self.base_addr + start * PAGE_SIZE;
                    unsafe {
                        core::ptr::write_bytes(addr as *mut u8, 0, count * PAGE_SIZE);
                    }
                    return Ok(addr);
                }
            } else {
                found_start = None;
                consecutive = 0;
            }
        }

        Err(KernelError::OutOfMemory)
    }

    pub fn free_page(&mut self, addr: PhysAddr) -> KernelResult<()> {
        if addr < self.base_addr || addr >= self.end_addr {
            return Err(KernelError::InvalidAddress);
        }
        if addr % PAGE_SIZE != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let index = (addr - self.base_addr) / PAGE_SIZE;
        if !self.get_bit(index) {
            return Err(KernelError::PageNotMapped);
        }

        self.clear_bit(index);
        self.free_page_count += 1;
        Ok(())
    }

    pub fn free_pages_range(&mut self, addr: PhysAddr, count: usize) -> KernelResult<()> {
        for i in 0..count {
            self.free_page(addr + i * PAGE_SIZE)?;
        }
        Ok(())
    }

    #[inline]
    fn get_bit(&self, index: usize) -> bool {
        let byte = index / 8;
        let bit = index % 8;
        if byte >= self.bitmap.len() {
            return true;
        }
        (self.bitmap[byte] >> bit) & 1 == 1
    }

    #[inline]
    fn set_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        if byte < self.bitmap.len() {
            self.bitmap[byte] |= 1 << bit;
        }
    }

    #[inline]
    fn clear_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        if byte < self.bitmap.len() {
            self.bitmap[byte] &= !(1 << bit);
        }
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn free_page_count(&self) -> usize {
        self.free_page_count
    }

    pub fn used_pages(&self) -> usize {
        self.total_pages - self.free_page_count
    }

    pub fn free_memory(&self) -> usize {
        self.free_page_count * PAGE_SIZE
    }

    pub fn used_memory(&self) -> usize {
        self.used_pages() * PAGE_SIZE
    }

    pub fn reserve_range(&mut self, start: PhysAddr, size: usize) {
        if start < self.base_addr {
            return;
        }
        let start_page = (start - self.base_addr) / PAGE_SIZE;
        let end_addr = start + size;
        let end_page = if end_addr > self.end_addr {
            self.total_pages
        } else {
            (end_addr - self.base_addr + PAGE_SIZE - 1) / PAGE_SIZE
        };

        for i in start_page..end_page {
            if i < self.total_pages && !self.get_bit(i) {
                self.set_bit(i);
                self.free_page_count = self.free_page_count.saturating_sub(1);
            }
        }
    }
}
