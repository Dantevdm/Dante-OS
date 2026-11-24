//! Physical memory (4 KiB frames) and virtual memory (page tables).
//!
//! Limine maps all of physical RAM at a fixed offset (the "higher-half direct
//! map", HHDM), so the kernel can reach any physical address `p` at `p + HHDM`.
//! On top of that we keep a frame allocator and can map new pages, e.g. for
//! the kernel heap or a device's registers.

use core::sync::atomic::{AtomicU64, Ordering};

use limine::memmap::{Entry, MEMMAP_USABLE};
use spin::Mutex;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::{MapToError, TranslateResult};
use x86_64::structures::paging::{
    FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags as Flags, PhysFrame,
    Size4KiB, Translate,
};
use x86_64::{PhysAddr, VirtAddr};

pub const PAGE: u64 = 4096;
static HHDM: AtomicU64 = AtomicU64::new(0);

pub fn phys_to_virt(p: u64) -> VirtAddr {
    VirtAddr::new(p + HHDM.load(Ordering::Relaxed))
}

// ---- Frame allocator ----

const MAX_REGIONS: usize = 64;

/// Hands out free 4 KiB frames. Fresh frames come from the usable regions in
/// order; freed frames go on a free list threaded through the frames themselves.
pub struct Frames {
    regions: [(u64, u64); MAX_REGIONS], // (start, end) physical, page aligned
    count: usize,
    region: usize,
    next: u64,
    free_list: u64, // physical address of the first freed frame, 0 if none
    pub total: u64,
    pub used: u64,
}

impl Frames {
    fn new(entries: &[&Entry]) -> Self {
        let mut f = Frames { regions: [(0, 0); MAX_REGIONS], count: 0, region: 0, next: 0, free_list: 0, total: 0, used: 0 };
        for e in entries.iter().filter(|e| e.type_ == MEMMAP_USABLE) {
            let start = e.base.div_ceil(PAGE) * PAGE;
            let end = (e.base + e.length) / PAGE * PAGE;
            // Skip the first page of RAM so that physical address 0 never looks like a valid frame.
            let start = start.max(PAGE);
            if end > start && f.count < MAX_REGIONS {
                f.regions[f.count] = (start, end);
                f.count += 1;
                f.total += (end - start) / PAGE;
            }
        }
        if f.count > 0 {
            f.next = f.regions[0].0;
        }
        f
    }

    pub fn alloc(&mut self) -> Option<u64> {
        if self.free_list != 0 {
            let frame = self.free_list;
            self.free_list = unsafe { phys_to_virt(frame).as_ptr::<u64>().read() };
            self.used += 1;
            return Some(frame);
        }
        while self.region < self.count {
            let (_, end) = self.regions[self.region];
            if self.next < end {
                let frame = self.next;
                self.next += PAGE;
                self.used += 1;
                return Some(frame);
            }
            self.region += 1;
            if self.region < self.count {
                self.next = self.regions[self.region].0;
            }
        }
        None
    }

    pub fn free(&mut self, frame: u64) {
        unsafe { phys_to_virt(frame).as_mut_ptr::<u64>().write(self.free_list) };
        self.free_list = frame;
        self.used -= 1;
    }

    /// `n` physically contiguous frames (for devices that read memory directly).
    pub fn alloc_contiguous(&mut self, n: u64) -> Option<u64> {
        while self.region < self.count {
            let (_, end) = self.regions[self.region];
            if self.next + n * PAGE <= end {
                let start = self.next;
                self.next += n * PAGE;
                self.used += n;
                return Some(start);
            }
            self.region += 1;
            if self.region < self.count {
                self.next = self.regions[self.region].0;
            }
        }
        None
    }

    /// Allocate a frame and fill it with zeros.
    pub fn alloc_zeroed(&mut self) -> Option<u64> {
        let f = self.alloc()?;
        unsafe { core::ptr::write_bytes(phys_to_virt(f).as_mut_ptr::<u8>(), 0, PAGE as usize) };
        Some(f)
    }
}

unsafe impl FrameAllocator<Size4KiB> for Frames {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        // Page tables must start zeroed.
        self.alloc_zeroed().map(|p| PhysFrame::containing_address(PhysAddr::new(p)))
    }
}

impl FrameDeallocator<Size4KiB> for Frames {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.free(frame.start_address().as_u64());
    }
}

pub static FRAMES: Mutex<Option<Frames>> = Mutex::new(None);
static MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);

pub fn init(hhdm: u64, entries: &[&Entry]) {
    HHDM.store(hhdm, Ordering::Relaxed);
    let frames = Frames::new(entries);
    crate::log!("memory: {} frames free ({} MiB)", frames.total, frames.total * PAGE / (1024 * 1024));
    *FRAMES.lock() = Some(frames);

    let (l4, _) = Cr3::read();
    let l4: &'static mut PageTable = unsafe { &mut *phys_to_virt(l4.start_address().as_u64()).as_mut_ptr() };
    *MAPPER.lock() = Some(unsafe { OffsetPageTable::new(l4, VirtAddr::new(hhdm)) });
}

/// Back the virtual range [start, start + size) with fresh, zeroed RAM.
pub fn map_ram(start: u64, size: u64) -> Result<(), MapToError<Size4KiB>> {
    let mut mapper = MAPPER.lock();
    let mapper = mapper.as_mut().unwrap();
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().unwrap();
    let flags = Flags::PRESENT | Flags::WRITABLE | Flags::NO_EXECUTE;
    for addr in (start..start + size).step_by(PAGE as usize) {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr));
        let frame = PhysFrame::containing_address(PhysAddr::new(frames.alloc_zeroed().ok_or(MapToError::FrameAllocationFailed)?));
        unsafe { mapper.map_to(page, frame, flags, frames)?.flush() };
    }
    Ok(())
}

/// Make a device's registers at physical `phys` reachable, uncached, through the HHDM.
pub fn map_mmio(phys: u64, size: u64) -> VirtAddr {
    let mut mapper = MAPPER.lock();
    let mapper = mapper.as_mut().unwrap();
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().unwrap();
    let flags = Flags::PRESENT | Flags::WRITABLE | Flags::NO_CACHE | Flags::WRITE_THROUGH | Flags::NO_EXECUTE;
    let base = phys / PAGE * PAGE;
    for p in (base..phys + size).step_by(PAGE as usize) {
        let virt = phys_to_virt(p);
        if let TranslateResult::NotMapped = mapper.translate(virt) {
            let page = Page::<Size4KiB>::containing_address(virt);
            let frame = PhysFrame::containing_address(PhysAddr::new(p));
            unsafe {
                if let Ok(flush) = mapper.map_to(page, frame, flags, frames) {
                    flush.flush();
                }
            }
        }
    }
    phys_to_virt(phys)
}

pub fn stats() -> (u64, u64) {
    FRAMES.lock().as_ref().map(|f| (f.used, f.total)).unwrap_or((0, 0))
}

/// Zeroed, physically contiguous memory for a device. Returns (physical, virtual).
pub fn dma_alloc(pages: u64) -> Option<(u64, *mut u8)> {
    let phys = FRAMES.lock().as_mut()?.alloc_contiguous(pages)?;
    let virt = phys_to_virt(phys).as_mut_ptr::<u8>();
    unsafe { core::ptr::write_bytes(virt, 0, (pages * PAGE) as usize) };
    Some((phys, virt))
}
