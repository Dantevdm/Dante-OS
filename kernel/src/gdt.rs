//! Global Descriptor Table and Task State Segment.
//!
//! The GDT describes the kernel's and user programs' code and data segments.
//! The TSS tells the CPU which stack to use when an interrupt arrives while a
//! program runs (`set_kernel_stack`), and gives the double-fault handler a
//! known-good stack so even a stack overflow ends on the crash screen.

use core::ptr::{addr_of, addr_of_mut};
use spin::LazyLock as Lazy;
use x86_64::VirtAddr;
use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST: u16 = 0;
const EMERGENCY_STACK_SIZE: usize = 5 * 4096;

#[repr(align(16))]
#[allow(dead_code)]
struct Stack([u8; EMERGENCY_STACK_SIZE]);
static mut EMERGENCY_STACK: Stack = Stack([0; EMERGENCY_STACK_SIZE]);

// Changed on every switch to a program, so it lives in a plain static.
static mut TSS: TaskStateSegment = TaskStateSegment::new();

struct Selectors {
    code: SegmentSelector,
    data: SegmentSelector,
    user_code: SegmentSelector,
    user_data: SegmentSelector,
    tss: SegmentSelector,
}

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let top = VirtAddr::from_ptr(&raw const EMERGENCY_STACK) + EMERGENCY_STACK_SIZE as u64;
    unsafe { (*addr_of_mut!(TSS)).interrupt_stack_table[DOUBLE_FAULT_IST as usize] = top };
    let mut gdt = GlobalDescriptorTable::new();
    let code = gdt.append(Descriptor::kernel_code_segment());
    let data = gdt.append(Descriptor::kernel_data_segment());
    let user_data = gdt.append(Descriptor::user_data_segment());
    let user_code = gdt.append(Descriptor::user_code_segment());
    let tss = gdt.append(Descriptor::tss_segment(unsafe { &*addr_of!(TSS) }));
    (gdt, Selectors { code, data, user_code, user_data, tss })
});

pub fn init() {
    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code);
        SS::set_reg(GDT.1.data);
        DS::set_reg(GDT.1.data);
        ES::set_reg(GDT.1.data);
        load_tss(GDT.1.tss);
    }
}

/// The code and stack selectors for user mode (ring 3).
pub fn user_selectors() -> (u64, u64) {
    (GDT.1.user_code.0 as u64 | 3, GDT.1.user_data.0 as u64 | 3)
}

/// The stack the CPU switches to when a program is interrupted.
pub fn set_kernel_stack(top: u64) {
    unsafe { (*addr_of_mut!(TSS)).privilege_stack_table[0] = VirtAddr::new(top) };
}
