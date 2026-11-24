//! The program's memory allocator. It asks the kernel for more memory as
//! needed, so `Vec`, `String` and `Box` work.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{NonNull, null_mut};
use linked_list_allocator::Heap;

use crate::sys;

const CHUNK: usize = 256 * 1024;

struct Allocator(UnsafeCell<Option<Heap>>);

// Programs have a single thread.
unsafe impl Sync for Allocator {}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator(UnsafeCell::new(None));

fn grow(bytes: usize) -> Option<*mut u8> {
    sys::check(sys::call(sys::GROW, bytes as u64, 0, 0, 0)).ok().map(|a| a as *mut u8)
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap = unsafe { &mut *self.0.get() };
        if heap.is_none() {
            let Some(start) = grow(CHUNK) else { return null_mut() };
            *heap = Some(unsafe { Heap::new(start, CHUNK) });
        }
        let heap = heap.as_mut().unwrap();
        loop {
            if let Ok(p) = heap.allocate_first_fit(layout) {
                return p.as_ptr();
            }
            // The kernel hands out memory right after the last piece, so the heap can extend.
            let more = (layout.size() + layout.align()).max(CHUNK).next_multiple_of(4096);
            if grow(more).is_none() {
                return null_mut();
            }
            unsafe { heap.extend(more) };
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(heap) = unsafe { &mut *self.0.get() } {
            unsafe { heap.deallocate(NonNull::new_unchecked(ptr), layout) };
        }
    }
}
