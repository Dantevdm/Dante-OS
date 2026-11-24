//! The APIC: how modern PCs deliver interrupts.
//!
//! Each CPU has a Local APIC (with its own timer); devices raise interrupts
//! through the I/O APIC, which routes them to a CPU. The old 8259 PIC is
//! remapped and switched off.

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::Msr;

use crate::acpi::Madt;
use crate::memory;

static LAPIC: AtomicU64 = AtomicU64::new(0);

// Local APIC registers (offsets from its base).
const ID: u64 = 0x20;
const EOI: u64 = 0xB0;
const SPURIOUS: u64 = 0xF0;
const LVT_TIMER: u64 = 0x320;
const TIMER_INITIAL: u64 = 0x380;
const TIMER_CURRENT: u64 = 0x390;
const TIMER_DIVIDE: u64 = 0x3E0;

pub const SPURIOUS_VECTOR: u8 = 0xFF;

fn lapic_write(reg: u64, v: u32) {
    unsafe { ((LAPIC.load(Ordering::Relaxed) + reg) as *mut u32).write_volatile(v) }
}

fn lapic_read(reg: u64) -> u32 {
    unsafe { ((LAPIC.load(Ordering::Relaxed) + reg) as *const u32).read_volatile() }
}

pub fn eoi() {
    lapic_write(EOI, 0);
}

pub fn lapic_id() -> u32 {
    lapic_read(ID) >> 24
}

fn disable_pic() {
    unsafe {
        let mut pics = pic8259::ChainedPics::new(0x20, 0x28);
        pics.initialize(); // move its vectors out of the way of CPU exceptions
        pics.write_masks(0xFF, 0xFF); // then silence it
    }
}

/// Start the Local APIC and its timer at `hz`, firing `timer_vector`.
pub fn init_local(timer_vector: u8, hz: u32) {
    disable_pic();
    let base = unsafe { Msr::new(0x1B).read() } & 0xFFFF_F000;
    LAPIC.store(memory::map_mmio(base, 4096).as_u64(), Ordering::Relaxed);
    lapic_write(SPURIOUS, 0x100 | SPURIOUS_VECTOR as u32); // enable

    // Measure how fast the APIC timer counts, using 10 ms on the PIT as a ruler.
    lapic_write(TIMER_DIVIDE, 0x3); // divide by 16
    let per_10ms = {
        lapic_write(TIMER_INITIAL, u32::MAX);
        pit_wait_10ms();
        u32::MAX - lapic_read(TIMER_CURRENT)
    };
    let per_tick = (per_10ms as u64 * 100 / hz as u64).max(1) as u32;
    lapic_write(LVT_TIMER, timer_vector as u32 | (1 << 17)); // periodic
    lapic_write(TIMER_INITIAL, per_tick);
    crate::log!("apic: local APIC id {} at {base:#x}, timer {hz} Hz ({per_tick} counts)", lapic_id());
}

/// Busy-wait 10 ms using PIT channel 2 (no interrupts needed).
fn pit_wait_10ms() {
    unsafe {
        let mut gate = Port::<u8>::new(0x61);
        let g = gate.read();
        gate.write((g & !0x02) | 0x01); // speaker off, gate on
        Port::<u8>::new(0x43).write(0b1011_0000); // channel 2, lo/hi, mode 0
        let count: u16 = 11932; // 1.193182 MHz * 10 ms
        Port::<u8>::new(0x42).write(count as u8);
        Port::<u8>::new(0x42).write((count >> 8) as u8);
        let g = gate.read();
        gate.write(g & !0x01);
        gate.write(g | 0x01); // restart the count
        while gate.read() & 0x20 == 0 {}
    }
}

// ---- I/O APIC ----

static IOAPIC: AtomicU64 = AtomicU64::new(0);

fn ioapic_write(reg: u32, v: u32) {
    let base = IOAPIC.load(Ordering::Relaxed);
    unsafe {
        (base as *mut u32).write_volatile(reg);
        ((base + 0x10) as *mut u32).write_volatile(v);
    }
}

pub fn init_io(madt: &Madt) {
    IOAPIC.store(memory::map_mmio(madt.ioapic_addr, 4096).as_u64(), Ordering::Relaxed);
    crate::log!("apic: I/O APIC at {:#x}", madt.ioapic_addr);
}

/// Route ISA `irq` (via any ACPI override) to `vector` on this CPU.
pub fn route_isa_irq(madt: &Madt, irq: u8, vector: u8) {
    let (gsi, flags) = madt.gsi_for_isa(irq);
    let pin = gsi - madt.ioapic_gsi_base;
    let mut low = vector as u32;
    if flags & 0b11 == 0b11 {
        low |= 1 << 13; // active low
    }
    if (flags >> 2) & 0b11 == 0b11 {
        low |= 1 << 15; // level triggered
    }
    ioapic_write(0x10 + pin * 2 + 1, lapic_id() << 24);
    ioapic_write(0x10 + pin * 2, low);
    crate::log!("apic: IRQ {irq} -> GSI {gsi} -> vector {vector}");
}
