use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};

const SLAB_MIN_SIZE: usize = 8;
const SLAB_MAX_SIZE: usize = 4096;
const SLAB_CLASSES: usize = 10;
const MAX_BLOCKS_PER_CLASS: usize = 1024;

static HEAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

#[global_allocator]
static mut KERNEL_HEAP: SlabAllocator = SlabAllocator::new();

pub type KernelHeap = SlabAllocator;

pub struct SlabAllocator {
    heap_start: usize,
    heap_size: usize,
    bump_offset: usize,
    slabs: [SlabClass; SLAB_CLASSES],
}

struct SlabClass {
    block_size: usize,
    free_list: [usize; MAX_BLOCKS_PER_CLASS],
    free_count: usize,
    total_count: usize,
    next_page: usize,
}

impl SlabClass {
    const fn new(block_size: usize) -> Self {
        SlabClass {
            block_size,
            free_list: [0; MAX_BLOCKS_PER_CLASS],
            free_count: 0,
            total_count: 0,
            next_page: 0,
        }
    }
}

impl SlabAllocator {
    const fn new() -> Self {
        const fn make_slab(idx: usize) -> SlabClass {
            SlabClass::new(SLAB_MIN_SIZE << idx)
        }

        SlabAllocator {
            heap_start: 0,
            heap_size: 0,
            bump_offset: 0,
            slabs: [
                make_slab(0), make_slab(1), make_slab(2), make_slab(3), make_slab(4),
                make_slab(5), make_slab(6), make_slab(7), make_slab(8), make_slab(9),
            ],
        }
    }

    pub unsafe fn init(start: usize, size: usize) {
        if HEAP_INITIALIZED.load(Ordering::SeqCst) {
            return;
        }

        KERNEL_HEAP.heap_start = start;
        KERNEL_HEAP.heap_size = size;
        KERNEL_HEAP.bump_offset = 0;

        let mut offset = 0;
        for i in 0..SLAB_CLASSES {
            let slab = &mut KERNEL_HEAP.slabs[i];
            let page_addr = start + offset;
            slab.next_page = page_addr;
            slab.free_count = 0;
            slab.total_count = 0;

            core::ptr::write_bytes(page_addr as *mut u8, 0, 4096);

            let blocks_per_page = 4096 / slab.block_size;
            for j in 0..blocks_per_page.min(MAX_BLOCKS_PER_CLASS) {
                slab.free_list[j] = page_addr + j * slab.block_size;
                slab.free_count += 1;
                slab.total_count += 1;
            }

            offset += 4096;
        }

        KERNEL_HEAP.bump_offset = offset;

        HEAP_INITIALIZED.store(true, Ordering::SeqCst);
    }

    fn find_slab_class(size: usize) -> Option<usize> {
        if size > SLAB_MAX_SIZE || size == 0 {
            return None;
        }
        for i in 0..SLAB_CLASSES {
            let class_size = SLAB_MIN_SIZE << i;
            if size <= class_size {
                return Some(i);
            }
        }
        None
    }

    fn alloc_from_slab(&mut self, class_idx: usize) -> *mut u8 {
        let slab = &mut self.slabs[class_idx];

        if slab.free_count > 0 {
            slab.free_count -= 1;
            let ptr = slab.free_list[slab.free_count];
            return ptr as *mut u8;
        }

        if self.bump_offset + 4096 > self.heap_size {
            return core::ptr::null_mut();
        }

        let new_page = self.heap_start + self.bump_offset;
        self.bump_offset += 4096;

        unsafe { core::ptr::write_bytes(new_page as *mut u8, 0, 4096); }

        let blocks_per_page = 4096 / slab.block_size;
        let mut added = 0;
        for j in 0..blocks_per_page.min(MAX_BLOCKS_PER_CLASS - slab.total_count) {
            slab.free_list[slab.free_count + added] = new_page + j * slab.block_size;
            added += 1;
            slab.total_count += 1;
        }
        slab.free_count += added;

        if slab.free_count > 0 {
            slab.free_count -= 1;
            let ptr = slab.free_list[slab.free_count];
            return ptr as *mut u8;
        }

        core::ptr::null_mut()
    }

    fn alloc_bump(&mut self, size: usize, align: usize) -> *mut u8 {
        let aligned_offset = (self.bump_offset + align - 1) & !(align - 1);
        let new_offset = aligned_offset + size;

        if new_offset > self.heap_size {
            return core::ptr::null_mut();
        }

        let ptr = self.heap_start + aligned_offset;
        self.bump_offset = new_offset;
        ptr as *mut u8
    }

    fn free_to_slab(&mut self, ptr: *mut u8, class_idx: usize) {
        let slab = &mut self.slabs[class_idx];
        let addr = ptr as usize;

        if slab.free_count < MAX_BLOCKS_PER_CLASS {
            slab.free_list[slab.free_count] = addr;
            slab.free_count += 1;
        }
    }
}

#[allow(invalid_reference_casting)]
unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        let aligned_size = if size < align { align } else { size };

        if let Some(class_idx) = Self::find_slab_class(aligned_size) {
            let slab = &self.slabs[class_idx];
            if slab.block_size >= align {
                return (&mut *(self as *const Self as *mut Self))
                    .alloc_from_slab(class_idx);
            }
        }

        (&mut *(self as *const Self as *mut Self))
            .alloc_bump(aligned_size, align)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let align = layout.align();
        let aligned_size = if size < align { align } else { size };

        if let Some(class_idx) = Self::find_slab_class(aligned_size) {
            (&mut *(self as *const Self as *mut Self))
                .free_to_slab(ptr, class_idx);
        }
    }
}

pub fn heap_stats() -> HeapStats {
    let mut stats = HeapStats::default();
    unsafe {
        stats.heap_start = KERNEL_HEAP.heap_start;
        stats.heap_size = KERNEL_HEAP.heap_size;
        stats.bump_used = KERNEL_HEAP.bump_offset;

        for i in 0..SLAB_CLASSES {
            stats.slab_used[i] = KERNEL_HEAP.slabs[i].total_count - KERNEL_HEAP.slabs[i].free_count;
            stats.slab_free[i] = KERNEL_HEAP.slabs[i].free_count;
            stats.slab_block_size[i] = KERNEL_HEAP.slabs[i].block_size;
        }
    }
    stats
}

#[derive(Debug, Default)]
pub struct HeapStats {
    pub heap_start: usize,
    pub heap_size: usize,
    pub bump_used: usize,
    pub slab_used: [usize; SLAB_CLASSES],
    pub slab_free: [usize; SLAB_CLASSES],
    pub slab_block_size: [usize; SLAB_CLASSES],
}
