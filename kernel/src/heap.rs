//! The kernel heap, so kernel code can use `Vec`, `String` and `Box`.

use linked_list_allocator::LockedHeap;

/// Where the heap lives in virtual memory: well away from the HHDM and the kernel.
pub const HEAP_START: u64 = 0xFFFF_A000_0000_0000;
pub const HEAP_SIZE: u64 = 64 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init() {
    crate::memory::map_ram(HEAP_START, HEAP_SIZE).expect("could not map the kernel heap");
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE as usize) };
    crate::log!("heap: {} MiB at {HEAP_START:#x}", HEAP_SIZE / (1024 * 1024));
}

/// (used, free) bytes.
pub fn stats() -> (usize, usize) {
    let h = ALLOCATOR.lock();
    (h.used(), h.free())
}
