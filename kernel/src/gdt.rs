//! Global Descriptor Table and Task State Segment.
//!
//! The GDT tells the CPU about our code/data segments. The TSS gives the CPU
//! a known-good stack to switch to when a double fault happens, so even a
//! stack overflow ends on the crash screen instead of a silent reboot.

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

static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    let top = VirtAddr::from_ptr(&raw const EMERGENCY_STACK) + EMERGENCY_STACK_SIZE as u64;
    tss.interrupt_stack_table[DOUBLE_FAULT_IST as usize] = top;
    tss
});

struct Selectors {
    code: SegmentSelector,
    data: SegmentSelector,
    tss: SegmentSelector,
}

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let code = gdt.append(Descriptor::kernel_code_segment());
    let data = gdt.append(Descriptor::kernel_data_segment());
    let tss = gdt.append(Descriptor::tss_segment(&TSS));
    (gdt, Selectors { code, data, tss })
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
